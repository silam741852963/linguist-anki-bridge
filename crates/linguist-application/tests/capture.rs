use linguist_application::capture::discover_media;
use std::collections::BTreeMap;

#[test]
fn html_entities_urls_and_sound_keep_field_associations_without_rewriting() {
    let fields = BTreeMap::from([
        ("Picture".into(), r#"<IMG SRC="猫%20%26犬.png"><img src='a&amp;b.png'><video poster=still.jpg><source src=clip.mp4></video>"#.into()),
        ("Original".into(), "[sound:猫 voice.mp3][sound:猫 voice.mp3]".into()),
        ("Unmapped".into(), "<audio src='猫%20voice.mp3'></audio>".into()),
    ]);
    let original = fields.clone();
    let result = discover_media(&fields, 10000, 100).unwrap();
    assert_eq!(fields, original);
    assert!(result.issues.is_empty());
    assert_eq!(result.references.len(), 7);
    assert_eq!(result.references[0].field, "Original");
    assert_eq!(result.references[0].filename, "猫 voice.mp3");
    assert_eq!(result.references[1], result.references[0]);
    assert!(result.references.iter().any(|r| r.filename == "猫 &犬.png"));
    assert!(result.references.iter().any(|r| r.filename == "a&b.png"));
    assert!(result.references.iter().any(|r| r.syntax == "video.poster"));
    assert!(
        result
            .references
            .iter()
            .any(|r| r.field == "Unmapped" && r.filename == "猫 voice.mp3")
    );
}

#[test]
fn remote_paths_and_unsupported_syntax_are_review_issues_not_fetches() {
    let fields = BTreeMap::from([(
        "Source".into(),
        r#"
      <img src="https://example.invalid/private"><img src="%2e%2e%2fsecret">
      <img src="file:///etc/passwd"><img src="//localhost/x"><img src="%ff.png">
      <img src="good.png" srcset="other.png 2x"><div style="background:url(hidden.png)"></div>
      <style>p {background:url(hidden2.png)}</style><object data="object.webm"></object>
      [sound:../secret.mp3][sound:unfinished
    "#
        .into(),
    )]);
    let result = discover_media(&fields, 10000, 100).unwrap();
    assert_eq!(
        result
            .references
            .iter()
            .map(|r| r.filename.as_str())
            .collect::<Vec<_>>(),
        vec!["good.png", "object.webm"]
    );
    assert!(
        result
            .issues
            .iter()
            .any(|i| i.code == "MALFORMED_SOUND_REFERENCE")
    );
    assert!(
        result
            .issues
            .iter()
            .any(|i| i.code == "INVALID_MEDIA_ENCODING")
    );
    assert_eq!(
        result
            .issues
            .iter()
            .filter(|i| i.code == "UNSUPPORTED_MEDIA_SYNTAX")
            .count(),
        3
    );
    assert_eq!(
        result
            .issues
            .iter()
            .filter(|i| i.code == "UNSAFE_OR_REMOTE_MEDIA_REFERENCE")
            .count(),
        5
    );
}

#[test]
fn percent_decoding_happens_once_and_case_collisions_remain_visible() {
    let fields = BTreeMap::from([(
        "F".into(),
        "<img src='x%252F.png'><img src='Photo.png'><img src='photo.png'>[sound:x%2F.png]".into(),
    )]);
    let result = discover_media(&fields, 1000, 100).unwrap();
    assert_eq!(
        result
            .references
            .iter()
            .filter(|r| r.filename == "x%2F.png")
            .count(),
        2
    );
    assert_eq!(
        result
            .issues
            .iter()
            .filter(|i| i.code == "SOURCE_MEDIA_CASE_COLLISION")
            .count(),
        2
    );
}

#[test]
fn discovery_is_bounded_and_plain_text_never_becomes_an_image_reference() {
    let fields = BTreeMap::from([(
        "F".into(),
        "An explanation of image.png and a URL https://example.invalid/image.png".into(),
    )]);
    assert!(
        discover_media(&fields, 1000, 100)
            .unwrap()
            .references
            .is_empty()
    );
    assert_eq!(
        discover_media(&fields, 1, 100).unwrap_err(),
        "CAPTURE_INPUT_LIMIT"
    );
    assert_eq!(
        discover_media(&fields, 0, 100).unwrap_err(),
        "CAPTURE_INVALID_LIMITS"
    );
    let fields = BTreeMap::from([("F".into(), "[sound:a.mp3][sound:a.mp3]".into())]);
    assert_eq!(
        discover_media(&fields, 1000, 1).unwrap_err(),
        "CAPTURE_REFERENCE_LIMIT"
    );
    let fields = BTreeMap::from([("F".into(), "[sound:unfinished".into())]);
    assert_eq!(discover_media(&fields, 1000, 1).unwrap().issues.len(), 1);
}

#[test]
fn colour_styles_are_not_media_but_resource_styles_still_are() {
    let fields = BTreeMap::from([(
        "Word".into(),
        r#"<span style="color: rgb(34, 34, 34);">俳優</span><b style="font-weight:bold">x</b>
           <i style="background: URL ( a.png )"></i><i style="b\61 ckground:u\72 l(b.png)"></i>
           <i style="/* c */ color:red"></i><i style="list-style-image:x"></i>"#
            .into(),
    )]);
    let result = discover_media(&fields, 10000, 100).unwrap();
    assert!(result.references.is_empty());
    assert_eq!(
        result
            .issues
            .iter()
            .filter(|i| i.code == "UNSUPPORTED_MEDIA_SYNTAX")
            .count(),
        4,
        "{:?}",
        result.issues
    );
}
