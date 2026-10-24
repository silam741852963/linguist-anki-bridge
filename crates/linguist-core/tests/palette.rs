//! WP-27 card colours: every part-of-speech class has a light and a dark
//! token pair in `vocabulary-v3.css`, readable against the panel (WCAG AA)
//! and apart from its common neighbours for colour-vision deficiencies.
const CSS: &str = include_str!("../../../resources/templates/vocabulary-v3.css");

fn hex(text: &str) -> [f64; 3] {
    let value = u32::from_str_radix(text.trim_start_matches('#'), 16).unwrap();
    [16, 8, 0].map(|shift| f64::from((value >> shift) & 0xff) / 255.0)
}

fn linear(c: [f64; 3]) -> [f64; 3] {
    c.map(|x| {
        if x <= 0.04045 {
            x / 12.92
        } else {
            ((x + 0.055) / 1.055).powf(2.4)
        }
    })
}

fn luminance(c: [f64; 3]) -> f64 {
    let [r, g, b] = linear(c);
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

fn contrast(a: &str, b: &str) -> f64 {
    let (a, b) = (luminance(hex(a)), luminance(hex(b)));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

/// The value of `--name` in each block that sets it, in file order (light
/// first, then dark).
fn token(name: &str) -> Vec<String> {
    let key = format!("--{name}: ");
    CSS.match_indices(&key)
        .map(|(at, _)| {
            CSS[at + key.len()..]
                .split([';', '}'])
                .next()
                .unwrap()
                .trim()
                .to_owned()
        })
        .collect()
}

const CLASSES: [&str; 11] = [
    "noun",
    "verb",
    "adjective",
    "adverb",
    "pronoun",
    "function",
    "number",
    "expression",
    "affix",
    "interjection",
    "unknown",
];

#[test]
fn every_class_is_readable_in_both_modes() {
    assert_eq!(&CLASSES[..10], linguist_core::profile::CLASSES);
    let panel = token("lab-panel");
    let ink = token("lab-ink");
    let on = token("lab-on-accent");
    let card = ["#ffffff", "#0d1117"];
    for class in CLASSES {
        let strong = token(&format!("lab-c-{class}"));
        let soft = token(&format!("lab-s-{class}"));
        assert_eq!((strong.len(), soft.len()), (2, 2), "{class}");
        assert!(
            CSS.contains(&format!("[data-tags~=\"lab::pos::{class}\"]")),
            "{class} has no accent rule"
        );
        for mode in 0..2 {
            let ratios = [
                (
                    "chip text on panel",
                    contrast(&strong[mode], &panel[mode]),
                    4.5,
                ),
                (
                    "chip text on soft",
                    contrast(&strong[mode], &soft[mode]),
                    4.5,
                ),
                ("ink on soft", contrast(&ink[mode], &soft[mode]), 4.5),
                ("number on accent", contrast(&on[mode], &strong[mode]), 4.5),
                ("border on card", contrast(&strong[mode], card[mode]), 3.0),
            ];
            for (what, ratio, minimum) in ratios {
                assert!(
                    ratio >= minimum,
                    "{class} mode {mode}: {what} {ratio:.2} < {minimum}"
                );
            }
        }
    }
}

/// Machado, Oliveira and Fernandes (2009), severity 1, on linear RGB.
const DEFICIENCIES: [(&str, [[f64; 3]; 3]); 3] = [
    (
        "protan",
        [
            [0.152286, 1.052583, -0.204868],
            [0.114503, 0.786281, 0.099216],
            [-0.003882, -0.048116, 1.051998],
        ],
    ),
    (
        "deutan",
        [
            [0.367322, 0.860646, -0.227968],
            [0.280085, 0.672501, 0.047413],
            [-0.011820, 0.042940, 0.968881],
        ],
    ),
    (
        "tritan",
        [
            [1.255528, -0.076749, -0.178779],
            [-0.078411, 0.930809, 0.147602],
            [0.004733, 0.691367, 0.303900],
        ],
    ),
];

fn lab(l: [f64; 3]) -> [f64; 3] {
    let x = (0.4124 * l[0] + 0.3576 * l[1] + 0.1805 * l[2]) / 0.95047;
    let y = 0.2126 * l[0] + 0.7152 * l[1] + 0.0722 * l[2];
    let z = (0.0193 * l[0] + 0.1192 * l[1] + 0.9505 * l[2]) / 1.08883;
    let f = |t: f64| {
        if t > 0.008856 {
            t.cbrt()
        } else {
            7.787 * t + 16.0 / 116.0
        }
    };
    let (fx, fy, fz) = (f(x), f(y), f(z));
    [116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz)]
}

fn simulated(colour: &str, m: &[[f64; 3]; 3]) -> [f64; 3] {
    let l = linear(hex(colour));
    lab([0, 1, 2].map(|i| (m[i][0] * l[0] + m[i][1] * l[1] + m[i][2] * l[2]).clamp(0.0, 1.0)))
}

#[test]
fn common_pairs_stay_apart_for_colour_vision_deficiencies() {
    for (a, b) in [("noun", "verb"), ("noun", "adjective"), ("verb", "adverb")] {
        let (first, second) = (token(&format!("lab-c-{a}")), token(&format!("lab-c-{b}")));
        for mode in 0..2 {
            for (name, matrix) in &DEFICIENCIES {
                let (p, q) = (
                    simulated(&first[mode], matrix),
                    simulated(&second[mode], matrix),
                );
                let distance = (0..3).map(|i| (p[i] - q[i]).powi(2)).sum::<f64>().sqrt();
                assert!(
                    distance >= 12.0,
                    "{a}/{b} mode {mode} {name}: ΔE {distance:.1}"
                );
            }
        }
    }
}
