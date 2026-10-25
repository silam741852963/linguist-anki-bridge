//! WP-25 harvest commands against a fixture AnkiConnect that only answers
//! reads: scan, list, show, the status commands and the export that
//! `vocab add --format jsonl` reads.
use serde_json::{Value, json};
use std::io::{BufRead, Read, Write};
use std::process::Command;

fn serve(listener: std::net::TcpListener) {
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let mut stream = stream.unwrap();
            let mut reader = std::io::BufReader::new(stream.try_clone().unwrap());
            let mut length = 0;
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" {
                    break;
                }
                if line.to_ascii_lowercase().starts_with("content-length:") {
                    length = line.split_once(':').unwrap().1.trim().parse().unwrap();
                }
            }
            let mut body = vec![0; length];
            reader.read_exact(&mut body).unwrap();
            let request: Value = serde_json::from_slice(&body).unwrap();
            let result = match request["action"].as_str().unwrap() {
                "getActiveProfile" => json!("Fixture"),
                "findNotes" => {
                    assert_eq!(request["params"]["query"], "deck:\"語彙\"");
                    json!([100, 101])
                }
                "notesInfo" => json!([
                    {"noteId":100,"modelName":"Picture Words","cards":[200],"tags":[],
                     "fields":{"Word":{"value":"パン","order":0},
                               "Example Sentence":{"value":"<div>毎朝パンを食べます。</div>","order":1}}},
                    {"noteId":101,"modelName":"Picture Words","cards":[201],"tags":[],
                     "fields":{"Word":{"value":"食べる","order":0},
                               "Example Sentence":{"value":"りんごを買いました。","order":1}}}
                ]),
                action => panic!("unexpected action {action}"),
            };
            let body = json!({"result":result,"error":null}).to_string();
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
        }
    });
}

#[test]
fn harvest_scans_ranks_marks_and_exports_vocab_add_records() {
    let root = std::env::temp_dir().join(format!("lab-harvest-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&root).unwrap();
    let config = root.join("config.toml");
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    std::fs::write(
        &config,
        format!(
            "[config]\nversion=2\n[anki]\nendpoint='http://{}'\n[storage]\nstate_dir='{1}/state'\ncache_dir='{1}/cache'\n[purposes.japanese_vocab]\nsource_deck='語彙'\n",
            listener.local_addr().unwrap(),
            root.display()
        ),
    )
    .unwrap();
    serve(listener);
    let run = |args: &[&str]| -> (i32, Value) {
        let output = Command::new(env!("CARGO_BIN_EXE_linguist-anki-bridge"))
            .env("HOME", "/tmp/lab-cli-test-no-home")
            .env_remove("XDG_CONFIG_HOME")
            .env_remove("LAB_CONFIG")
            .args(["--output", "json", "--config", config.to_str().unwrap()])
            .args(args)
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        let value = serde_json::from_str(&stdout).unwrap_or_else(|_| {
            panic!(
                "{args:?}: {stdout} {}",
                String::from_utf8_lossy(&output.stderr)
            )
        });
        (output.status.code().unwrap(), value)
    };

    let (code, scan) = run(&["harvest", "scan", "--purpose", "japanese_vocab"]);
    assert_eq!(code, 0, "{scan}");
    assert_eq!(scan["deck"], "語彙");
    assert_eq!(scan["deck_notes"], 2);
    assert_eq!(scan["from_fields"], 2);
    assert_eq!(scan["anki_writes"], false);

    let (_, list) = run(&["harvest", "list", "--purpose", "japanese_vocab"]);
    let words: Vec<&str> = list["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["word"].as_str().unwrap())
        .collect();
    for word in ["毎朝", "りんご", "買う"] {
        assert!(words.contains(&word), "{word} in {words:?}");
    }
    assert!(!words.contains(&"パン") && !words.contains(&"食べる"));
    let maiasa = &list["candidates"][words.iter().position(|w| *w == "毎朝").unwrap()];
    assert_eq!(maiasa["i_plus_one"], 1);
    assert_eq!(maiasa["sentence"], "毎朝パンを食べます。");

    let (_, show) = run(&["harvest", "show", "--purpose", "japanese_vocab", "買う"]);
    assert_eq!(show["contexts"][0]["note"], "101");
    assert_eq!(show["contexts"][0]["other_unknown"], json!(["りんご"]));

    let (code, ignored) = run(&[
        "harvest",
        "ignore",
        "--purpose",
        "japanese_vocab",
        "りんご",
        "無い",
    ]);
    assert_eq!(code, 1);
    assert_eq!(ignored["changed"], json!(["りんご"]));
    assert_eq!(ignored["not_found"], json!(["無い"]));
    // With りんご ignored, 買う is alone in its sentence.
    let (_, show) = run(&["harvest", "show", "--purpose", "japanese_vocab", "買う"]);
    assert_eq!(show["i_plus_one"], 1);
    let (_, list) = run(&[
        "harvest",
        "list",
        "--purpose",
        "japanese_vocab",
        "--status",
        "ignored",
    ]);
    assert_eq!(list["total"], 1);
    let (code, _) = run(&[
        "harvest",
        "unignore",
        "--purpose",
        "japanese_vocab",
        "りんご",
    ]);
    assert_eq!(code, 0);
    let (code, _) = run(&["harvest", "known", "--purpose", "japanese_vocab", "りんご"]);
    assert_eq!(code, 0);

    let (code, queued) = run(&[
        "harvest",
        "queue",
        "--purpose",
        "japanese_vocab",
        "--top",
        "1",
    ]);
    assert_eq!(code, 0);
    let queued_word = queued["changed"][0].as_str().unwrap().to_owned();
    let out = root.join("harvest.jsonl");
    let (code, export) = run(&[
        "harvest",
        "export",
        "--purpose",
        "japanese_vocab",
        "--output",
        out.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{export}");
    assert_eq!(export["records"], 1);
    let text = std::fs::read_to_string(&out).unwrap();
    let record: Value = serde_json::from_str(text.lines().next().unwrap()).unwrap();
    assert_eq!(record["body"]["expression"], queued_word.as_str());
    let input: linguist_application::AddInput = serde_json::from_value(record).unwrap();
    assert!(matches!(
        input,
        linguist_application::AddInput::Vocabulary { .. }
    ));
    // The file is never overwritten.
    let output = Command::new(env!("CARGO_BIN_EXE_linguist-anki-bridge"))
        .env("HOME", "/tmp/lab-cli-test-no-home")
        .args(["--config", config.to_str().unwrap(), "harvest", "export"])
        .args([
            "--purpose",
            "japanese_vocab",
            "--output",
            out.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("HARVEST_EXPORT_UNWRITABLE"));
    std::fs::remove_dir_all(root).unwrap();
}
