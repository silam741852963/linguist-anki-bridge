#!/usr/bin/env python3
"""Write the EV-11 deterministic semantic corpus (120 annotated fixtures).

The fixtures are authored here as compact tables so that each expectation sits
next to its rationale. The output is
`crates/linguist-application/tests/fixtures/semantic/corpus-v1.json`, checked
by `crates/linguist-application/tests/semantic_corpus.rs`. Re-run after
editing; the test fails when the committed corpus differs from this script.

Workflows: vocab_add, vocab_revamp, grammar_add, grammar_revamp; 30 each,
15 Japanese and 15 English. Every fixture states the readiness and issue codes
the policy requires. No fixture relies on a network service, an LLM or OCR:
dictionary fixtures carry canned provider bytes and media fixtures describe
bytes the test builds.
"""

import json
from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / "crates/linguist-application/tests/fixtures/semantic/corpus-v1.json"

FIXTURES = []


def ex(sentence, translation, provenance="user"):
    return {"sentence": sentence, "translation": translation, "provenance": provenance, "evidence_ids": []}


def add(fid, workflow, language, cats, rationale, body, expect, explanation=None, tasks=None,
        dictionary=None, extra=None):
    kind = "vocabulary" if workflow == "vocab_add" else "grammar"
    record = {"schema_version": 2, "kind": kind, "target_language": language, "body": body}
    if explanation:
        record["explanation_language"] = explanation
    if tasks:
        record["requested_tasks"] = tasks
    record.update(extra or {})
    purpose = {"vocab_add": "vocab", "grammar_add": "grammar"}[workflow]
    purpose = ("japanese_" if language == "ja" else "english_") + purpose
    FIXTURES.append({
        "id": fid, "workflow": workflow, "purpose": purpose, "language": language,
        "explanation_language": explanation or ("en" if workflow == "vocab_add" or language == "en" else "vi"),
        "categories": cats, "rationale": rationale, "input": record,
        "dictionary": dictionary, "settings": {}, "expect": expect,
    })


def revamp(fid, workflow, language, cats, rationale, model, fields, mapping, expect,
           media=None, tags=None, settings=None, reps=3):
    purpose = ("japanese_" if language == "ja" else "english_") + ("vocab" if workflow == "vocab_revamp" else "grammar")
    settings = dict(settings or {})
    explanation = settings.get("learning.explanation_language") or (
        "vi" if purpose == "japanese_grammar" else "en")
    FIXTURES.append({
        "id": fid, "workflow": workflow, "purpose": purpose, "language": language,
        "explanation_language": explanation, "categories": cats, "rationale": rationale,
        "input": {"model": model, "fields": fields, "mapping": mapping, "tags": tags or ["legacy"],
                  "reps": reps, "media": media or {}},
        "dictionary": None, "settings": settings, "expect": expect,
    })


def ready(**more):
    return {"ready": True, "issues": [], **more}


def blocked(*codes, **more):
    return {"ready": False, "issues": list(codes), **more}


JISHO_EMPTY = {"provider": "jisho", "body": {"meta": {"status": 200}, "data": []}}
JISHO_NAMA = {"provider": "jisho", "body": {"meta": {"status": 200}, "data": [
    {"slug": "生", "japanese": [{"word": "生", "reading": "なま"}],
     "senses": [{"english_definitions": ["raw", "uncooked"]}, {"english_definitions": ["draft (beer)"]}]},
    {"slug": "生-1", "japanese": [{"word": "生", "reading": "せい"}],
     "senses": [{"english_definitions": ["life", "living"]}]}]}}
JISHO_TABERU = {"provider": "jisho", "body": {"meta": {"status": 200}, "data": [
    {"slug": "食べる", "japanese": [{"word": "食べる", "reading": "たべる"}],
     "senses": [{"english_definitions": ["to eat"]}]}]}}
WIKI_FOREIGN_ONLY = {"provider": "wiktionary", "body": {"fr": [{"language": "French", "partOfSpeech": "Noun",
                                                               "definitions": [{"definition": "pain (bread)"}]}]}}
WIKI_BANK = {"provider": "wiktionary", "body": {"en": [
    {"language": "English", "partOfSpeech": "Noun", "definitions": [
        {"definition": "An institution where one can place and borrow money."},
        {"definition": "The edge of a river or lake."}]}]}}

# ---------------------------------------------------------------- vocab_add / ja
L = "ja"
W = "vocab_add"
add("VA-JA-01", W, L, ["kanji", "baseline"], "Kanji verb with okurigana; the reading is the pronunciation.",
    {"expression": "食べる", "meaning": "to eat", "sense_key": "eat", "reading": "たべる"},
    ready(rendered_contains={"Expression": "食べる", "Pronunciation": "たべる", "Meaning": "to eat"}))
add("VA-JA-02", W, L, ["kana"], "Kana-only expression keeps kana; no kanji is invented.",
    {"expression": "ありがとう", "meaning": "thank you", "sense_key": "thanks", "reading": "ありがとう"},
    ready(rendered_contains={"Expression": "ありがとう"}, rendered_excludes={"Kanji": "有"}))
add("VA-JA-03", W, L, ["kanji", "homograph"], "はし as bridge: the sense key is kept but never shown.",
    {"expression": "橋", "meaning": "bridge", "sense_key": "bridge-sense", "reading": "はし"},
    ready(rendered_contains={"Meaning": "bridge"}, rendered_excludes={"Meaning": "bridge-sense"}))
add("VA-JA-04", W, L, ["kanji", "homograph"], "はし as chopsticks: same reading, different sense.",
    {"expression": "箸", "meaning": "chopsticks", "sense_key": "chopsticks-sense", "reading": "はし"},
    ready(rendered_contains={"Meaning": "chopsticks"}, rendered_excludes={"Meaning": "chopsticks-sense"}))
add("VA-JA-05", W, L, ["kanji", "vietnamese_explanation"], "Vietnamese explanation is kept verbatim.",
    {"expression": "勉強", "meaning": "học tập; việc học", "sense_key": "study", "reading": "べんきょう"},
    ready(rendered_contains={"Meaning": "học tập; việc học"}), explanation="vi")
add("VA-JA-06", W, L, ["kanji", "task_leakage"], "The Production front masks the answer inside the meaning.",
    {"expression": "飲む", "meaning": "to drink (飲む)", "sense_key": "drink"},
    ready(rendered_contains={"Meaning": "〜"}, rendered_excludes={"Meaning": "飲む"}),
    tasks=["comprehension", "production"])
add("VA-JA-07", W, L, ["kana", "task_leakage"], "A kana-only word cannot be a Spelling card: its pronunciation is the answer.",
    {"expression": "すごい", "meaning": "amazing", "sense_key": "amazing", "reading": "すごい"},
    blocked("ANSWER_LEAK"), tasks=["comprehension", "spelling"])
add("VA-JA-08", W, L, ["kanji", "task_leakage"], "Spelling without a pronunciation or audio blocks.",
    {"expression": "飲む", "meaning": "to drink", "sense_key": "drink"},
    blocked("SPELLING_CUE_MISSING"), tasks=["comprehension", "spelling"])
add("VA-JA-09", W, L, ["kanji"], "Missing core meaning blocks; nothing is filled in.",
    {"expression": "走る", "sense_key": "run", "reading": "はしる"}, blocked("REQUIRED_CONTENT"))
add("VA-JA-10", W, L, ["kanji", "adversarial"], "HTML/script in authored text is escaped when rendered.",
    {"expression": "見る", "meaning": "<img src=x onerror=alert(1)>to see", "sense_key": "see"},
    ready(rendered_excludes={"Meaning": "<img"}))
add("VA-JA-11", W, L, ["kanji"], "A generated example without evidence is an unsupported claim.",
    {"expression": "書く", "meaning": "to write", "sense_key": "write",
     "examples": [ex("手紙を書きます。", "I write a letter.", "generated")]},
    blocked("GENERATED_EXAMPLE_EVIDENCE_REQUIRED"))
add("VA-JA-12", W, L, ["kanji", "vietnamese_explanation"], "User example with Vietnamese translation.",
    {"expression": "水", "meaning": "nước", "sense_key": "water", "reading": "みず",
     "examples": [ex("水を飲みます。", "Tôi uống nước.")]},
    ready(rendered_contains={"UsageExamples": "を飲みます。"}), explanation="vi")
add("VA-JA-13", W, L, ["kana", "no_dictionary_match"], "No dictionary entry: authored meaning stays, warning only.",
    {"expression": "ぴえん", "meaning": "(slang) on the verge of tears", "sense_key": "teary"},
    ready(issues=["DICTIONARY_NOT_FOUND"]), dictionary=JISHO_EMPTY)
add("VA-JA-14", W, L, ["kanji", "homograph"], "Two dictionary entries (なま/せい): a sense must be chosen.",
    {"expression": "生"}, blocked("DICTIONARY_SENSE_REVIEW"), dictionary=JISHO_NAMA)
add("VA-JA-15", W, L, ["kanji", "task_leakage"], "The Spelling front shows the reading, never the written form.",
    {"expression": "猫", "meaning": "cat", "sense_key": "cat", "reading": "ねこ"},
    ready(rendered_contains={"Pronunciation": "ねこ"}, rendered_excludes={"Meaning": "猫"}),
    tasks=["comprehension", "spelling"])

# ---------------------------------------------------------------- vocab_add / en
L = "en"
add("VA-EN-01", W, L, ["baseline"], "Plain English verb.",
    {"expression": "run", "meaning": "to move quickly on foot", "sense_key": "move-fast"},
    ready(rendered_contains={"Expression": "run"}))
add("VA-EN-02", W, L, ["homograph"], "bank (finance) is distinct from bank (river).",
    {"expression": "bank", "meaning": "an institution that keeps money", "sense_key": "finance"},
    ready(rendered_contains={"Meaning": "keeps money"}, rendered_excludes={"Meaning": "finance"}))
add("VA-EN-03", W, L, ["homograph"], "bank (river) has its own sense key.",
    {"expression": "bank", "meaning": "the land beside a river", "sense_key": "river"},
    ready(rendered_contains={"Meaning": "beside a river"}))
add("VA-EN-04", W, L, ["homograph"], "lead (metal) with its own pronunciation.",
    {"expression": "lead", "meaning": "a heavy grey metal", "sense_key": "metal", "pronunciation": "/lɛd/"},
    ready(rendered_contains={"Pronunciation": "/lɛd/"}))
add("VA-EN-05", W, L, ["vietnamese_explanation"], "English word explained in Vietnamese.",
    {"expression": "necessary", "meaning": "cần thiết", "sense_key": "needed"},
    ready(rendered_contains={"Meaning": "cần thiết"}), explanation="vi")
add("VA-EN-06", W, L, ["task_leakage"], "The Spelling front shows the pronunciation.",
    {"expression": "necessary", "meaning": "needed", "sense_key": "needed", "pronunciation": "/ˈnɛsəsɛri/"},
    ready(rendered_contains={"Pronunciation": "/ˈnɛsəsɛri/"}), tasks=["comprehension", "spelling"])
add("VA-EN-07", W, L, ["task_leakage"], "English Spelling without a pronunciation or audio blocks.",
    {"expression": "necessary", "meaning": "needed", "sense_key": "needed"},
    blocked("SPELLING_CUE_MISSING"), tasks=["comprehension", "spelling"])
add("VA-EN-08", W, L, ["task_leakage"], "A multi-word answer quoted in the meaning is masked on the front.",
    {"expression": "give up", "meaning": "to stop trying (give up)", "sense_key": "quit"},
    ready(rendered_contains={"Meaning": "〜"}, rendered_excludes={"Meaning": "give up"}),
    tasks=["comprehension", "production"])
add("VA-EN-09", W, L, ["task_leakage"], "Multi-word production front shows only the definition.",
    {"expression": "give up", "meaning": "to stop trying", "sense_key": "quit"},
    ready(rendered_contains={"Meaning": "to stop trying"}), tasks=["comprehension", "production"])
add("VA-EN-10", W, L, ["adversarial"], "Script in the expression is escaped, not executed.",
    {"expression": "<script>alert(1)</script>run", "meaning": "to move fast", "sense_key": "move"},
    ready(rendered_excludes={"Expression": "<script"}))
add("VA-EN-11", W, L, ["adversarial"], "Template braces in notes stay literal text.",
    {"expression": "walk", "meaning": "to move on foot {{Expression}}", "sense_key": "walk"},
    ready(rendered_contains={"Meaning": "{{Expression}}"}),
    extra={"personal_notes": "{{Expression}} {{#Meaning}}x{{/Meaning}}"})
add("VA-EN-12", W, L, ["no_dictionary_match"], "Only a French section exists: no English fallback.",
    {"expression": "pain", "meaning": "physical suffering", "sense_key": "hurt"},
    ready(issues=["DICTIONARY_NOT_FOUND"]), dictionary=WIKI_FOREIGN_ONLY)
add("VA-EN-13", W, L, ["homograph"], "One entry with two senses and no authored choice needs review.",
    {"expression": "bank"}, blocked("DICTIONARY_SENSE_REVIEW"), dictionary=WIKI_BANK)
add("VA-EN-14", W, L, ["baseline"], "Example in the target language with an English gloss.",
    {"expression": "borrow", "meaning": "to take something to return later", "sense_key": "take-temporarily",
     "examples": [ex("Can I borrow your pen?", "May I use your pen and give it back?")]},
    ready(rendered_contains={"UsageExamples": "your pen?"}))
add("VA-EN-15", W, L, ["adversarial"], "Context and source summary keep control characters out of fields.",
    {"expression": "light", "meaning": "not heavy", "sense_key": "weight"},
    ready(rendered_contains={"Meaning": "not heavy"}),
    extra={"context": "Seen in: \"a light bag\" <b>bold</b>", "source_summary": "Textbook p.12"})

# ---------------------------------------------------------------- grammar_add / ja
W = "grammar_add"
L = "ja"
RP = "Which pattern fits this sentence?"
add("GA-JA-01", W, L, ["kanji", "baseline"], "Permission pattern with one example.",
    {"pattern": "〜てもいい", "use_key": "permission", "meaning": "may; it is all right to",
     "formation": "V-て + もいい", "recognition_prompt": RP,
     "examples": [ex("ここで写真を撮ってもいいですか。", "May I take photos here?")]},
    ready(rendered_contains={"Pattern": "〜てもいい"}), explanation="en")
add("GA-JA-02", W, L, ["vietnamese_explanation", "kana"], "Japanese grammar explained in Vietnamese (preset).",
    {"pattern": "〜ながら", "use_key": "simultaneous", "meaning": "vừa ... vừa ...",
     "formation": "V-ます語幹 + ながら", "recognition_prompt": RP,
     "examples": [ex("音楽を聞きながら勉強します。", "Tôi vừa nghe nhạc vừa học.")]},
    ready(rendered_contains={"Meaning": "vừa ... vừa ...", "ExplanationLanguage": "vi"}))
add("GA-JA-03", W, L, ["kanji"], "Obligation pattern.",
    {"pattern": "〜なければならない", "use_key": "obligation", "meaning": "must; have to",
     "formation": "V-ない形(ない→なければ) + ならない", "recognition_prompt": RP,
     "examples": [ex("明日早く起きなければならない。", "I have to get up early tomorrow.")]},
    ready(), explanation="en")
add("GA-JA-04", W, L, ["kanji"], "Experience pattern.",
    {"pattern": "〜たことがある", "use_key": "experience", "meaning": "have done (at least once)",
     "formation": "V-た + ことがある", "recognition_prompt": RP,
     "examples": [ex("日本に行ったことがあります。", "I have been to Japan.")]},
    ready(), explanation="en")
add("GA-JA-05", W, L, ["homograph", "kana"], "ように (purpose) is one use of a homograph pattern.",
    {"pattern": "〜ように", "use_key": "purpose", "meaning": "so that",
     "formation": "V-辞書形/ない形 + ように", "recognition_prompt": RP,
     "examples": [ex("忘れないようにメモします。", "I take notes so that I won't forget.")]},
    ready(rendered_contains={"UseKey": "purpose"}), explanation="en")
add("GA-JA-06", W, L, ["homograph", "kana"], "ように (resemblance) is a separate use key.",
    {"pattern": "〜ように", "use_key": "resemblance", "meaning": "like; as",
     "formation": "N + の + ように", "recognition_prompt": RP,
     "examples": [ex("雪のように白い。", "White like snow.")]},
    ready(rendered_contains={"UseKey": "resemblance"}), explanation="en")
add("GA-JA-07", W, L, ["task_leakage", "kanji"], "Application exercise whose answer is not in the prompt.",
    {"pattern": "〜ながら", "use_key": "simultaneous", "meaning": "while doing",
     "formation": "V-ます語幹 + ながら", "recognition_prompt": RP,
     "exercise_prompt": "コーヒーを（のむ）＿＿＿＿新聞を読みます。", "exercise_answer": "飲みながら",
     "examples": [ex("歩きながら話す。", "Talk while walking.")]},
    ready(rendered_contains={"ExerciseAnswer": "飲みながら"}), explanation="en",
    tasks=["recognition", "application"])
add("GA-JA-08", W, L, ["task_leakage"], "Application without an answer blocks.",
    {"pattern": "〜ながら", "use_key": "simultaneous", "meaning": "while doing",
     "formation": "V-ます語幹 + ながら", "recognition_prompt": RP,
     "exercise_prompt": "テレビを見＿＿＿＿ご飯を食べる。",
     "examples": [ex("歩きながら話す。", "Talk while walking.")]},
    blocked("MISSING_EXERCISE"), explanation="en", tasks=["recognition", "application"])
add("GA-JA-09", W, L, ["task_leakage"], "Application prompt that contains its answer is rejected.",
    {"pattern": "〜ながら", "use_key": "simultaneous", "meaning": "while doing",
     "formation": "V-ます語幹 + ながら", "recognition_prompt": RP,
     "exercise_prompt": "テレビを見ながらご飯を食べる。（見ながら）", "exercise_answer": "見ながら",
     "examples": [ex("歩きながら話す。", "Talk while walking.")]},
    blocked("ANSWER_LEAK"), explanation="en", tasks=["recognition", "application"])
add("GA-JA-10", W, L, ["kanji"], "Grammar needs at least one validated use example.",
    {"pattern": "〜すぎる", "use_key": "excess", "meaning": "too much", "formation": "V-ます語幹/A語幹 + すぎる",
     "recognition_prompt": RP, "examples": []},
    blocked("GRAMMAR_EXAMPLE_REQUIRED"), explanation="en")
add("GA-JA-11", W, L, ["adversarial"], "Markup in the formation is escaped.",
    {"pattern": "〜たら", "use_key": "conditional", "meaning": "if; when",
     "formation": "<script>alert('x')</script>V-た + ら", "recognition_prompt": RP,
     "examples": [ex("雨が降ったら行きません。", "If it rains, I won't go.")]},
    ready(rendered_excludes={"Formation": "<script"}), explanation="en")
add("GA-JA-12", W, L, ["kana"], "An empty recognition prompt is missing core content.",
    {"pattern": "〜けど", "use_key": "contrast", "meaning": "but; although", "formation": "普通形 + けど",
     "recognition_prompt": "", "examples": [ex("高いけど、買います。", "It's expensive, but I'll buy it.")]},
    blocked("REQUIRED_CONTENT"), explanation="en")
add("GA-JA-13", W, L, ["kanji"], "Generated grammar example without evidence is unsupported.",
    {"pattern": "〜ば", "use_key": "conditional", "meaning": "if", "formation": "V-ば形",
     "recognition_prompt": RP, "examples": [ex("安ければ買います。", "If it's cheap, I'll buy it.", "generated")]},
    blocked("GENERATED_EXAMPLE_EVIDENCE_REQUIRED"), explanation="en")
add("GA-JA-14", W, L, ["kana", "baseline"], "Kana-only pattern with two examples.",
    {"pattern": "〜けど", "use_key": "contrast", "meaning": "but; although", "formation": "普通形 + けど",
     "recognition_prompt": RP, "examples": [ex("高いけど、買います。", "It's expensive, but I'll buy it."),
                                            ex("行きたいけど、時間がない。", "I want to go, but I have no time.")]},
    ready(rendered_contains={"Examples": "行きたいけど"}), explanation="en")
add("GA-JA-15", W, L, ["vietnamese_explanation", "kanji"], "Vietnamese preset with usage note.",
    {"pattern": "〜はずだ", "use_key": "expectation", "meaning": "chắc là; lẽ ra",
     "formation": "普通形 + はずだ", "usage": "Dựa trên căn cứ khách quan.", "recognition_prompt": RP,
     "examples": [ex("彼はもう着いたはずだ。", "Chắc là anh ấy đã đến rồi.")]},
    ready(rendered_contains={"Usage": "Dựa trên căn cứ khách quan."}))

# ---------------------------------------------------------------- grammar_add / en
L = "en"
RP = "Which structure is used here?"
add("GA-EN-01", W, L, ["baseline"], "Past habit.",
    {"pattern": "used to + V", "use_key": "past-habit", "meaning": "a past habit or state",
     "formation": "used to + base verb", "recognition_prompt": RP,
     "examples": [ex("I used to live in Hanoi.", "I lived in Hanoi in the past.")]},
    ready(rendered_contains={"Pattern": "used to + V"}))
add("GA-EN-02", W, L, ["baseline"], "Present perfect experience.",
    {"pattern": "have + past participle", "use_key": "experience", "meaning": "life experience up to now",
     "formation": "have/has + V3", "recognition_prompt": RP,
     "examples": [ex("I have seen that film.", "At some time I saw that film.")]},
    ready())
add("GA-EN-03", W, L, ["baseline"], "Advice with had better.",
    {"pattern": "had better + V", "use_key": "strong-advice", "meaning": "it is advisable to",
     "formation": "had better + base verb", "recognition_prompt": RP,
     "examples": [ex("You had better leave now.", "You should leave now.")]},
    ready())
add("GA-EN-04", W, L, ["vietnamese_explanation"], "English grammar explained in Vietnamese.",
    {"pattern": "be going to + V", "use_key": "plan", "meaning": "sắp; dự định",
     "formation": "am/is/are going to + V", "recognition_prompt": RP,
     "examples": [ex("I am going to visit my aunt.", "Tôi định thăm dì.")]},
    ready(rendered_contains={"Meaning": "sắp; dự định"}), explanation="vi")
add("GA-EN-05", W, L, ["multi_pattern"], "A correlative structure is one pattern, not two cards.",
    {"pattern": "not only ... but also ...", "use_key": "addition", "meaning": "both, with emphasis",
     "formation": "not only X but also Y", "recognition_prompt": RP,
     "examples": [ex("She speaks not only English but also Japanese.", "She speaks both languages.")]},
    ready(rendered_contains={"Pattern": "not only ... but also ..."}))
add("GA-EN-06", W, L, ["homograph"], "would (past habit) is a separate use from would (conditional).",
    {"pattern": "would + V", "use_key": "past-habit", "meaning": "repeated past action",
     "formation": "would + base verb", "recognition_prompt": RP,
     "examples": [ex("Every summer we would swim in the lake.", "We swam there each summer.")]},
    ready(rendered_contains={"UseKey": "past-habit"}))
add("GA-EN-07", W, L, ["homograph"], "would (conditional) has its own use key.",
    {"pattern": "would + V", "use_key": "conditional", "meaning": "result of an unreal condition",
     "formation": "if + past, would + base verb", "recognition_prompt": RP,
     "examples": [ex("If I had time, I would help.", "I lack time, so I can't help.")]},
    ready(rendered_contains={"UseKey": "conditional"}))
add("GA-EN-08", W, L, ["task_leakage"], "Application exercise with a hidden answer.",
    {"pattern": "used to + V", "use_key": "past-habit", "meaning": "a past habit",
     "formation": "used to + base verb", "recognition_prompt": RP,
     "exercise_prompt": "I ___ play tennis as a child. (habit)", "exercise_answer": "used to",
     "examples": [ex("I used to live in Hanoi.", "I lived in Hanoi in the past.")]},
    ready(rendered_contains={"ExercisePrompt": "I ___ play tennis"}), tasks=["recognition", "application"])
add("GA-EN-09", W, L, ["task_leakage"], "Application exercise without an answer blocks.",
    {"pattern": "used to + V", "use_key": "past-habit", "meaning": "a past habit",
     "formation": "used to + base verb", "recognition_prompt": RP,
     "exercise_prompt": "I ___ play tennis as a child.",
     "examples": [ex("I used to live in Hanoi.", "I lived in Hanoi in the past.")]},
    blocked("MISSING_EXERCISE"), tasks=["recognition", "application"])
add("GA-EN-10", W, L, ["task_leakage"], "Exercise prompt that states the answer is rejected.",
    {"pattern": "used to + V", "use_key": "past-habit", "meaning": "a past habit",
     "formation": "used to + base verb", "recognition_prompt": RP,
     "exercise_prompt": "I used to play tennis. Fill: I ___ play tennis.", "exercise_answer": "used to",
     "examples": [ex("I used to live in Hanoi.", "I lived in Hanoi in the past.")]},
    blocked("ANSWER_LEAK"), tasks=["recognition", "application"])
add("GA-EN-11", W, L, ["adversarial"], "Event-handler markup in the meaning is escaped.",
    {"pattern": "so ... that", "use_key": "result", "meaning": "<a href=javascript:alert(1)>so much that</a>",
     "formation": "so + adj + that + clause", "recognition_prompt": RP,
     "examples": [ex("It was so cold that we stayed in.", "Because it was very cold, we stayed in.")]},
    ready(rendered_excludes={"Meaning": "<a "}))
add("GA-EN-12", W, L, ["baseline"], "No examples blocks grammar readiness.",
    {"pattern": "would rather + V", "use_key": "preference", "meaning": "prefer to",
     "formation": "would rather + base verb", "recognition_prompt": RP, "examples": []},
    blocked("GRAMMAR_EXAMPLE_REQUIRED"))
add("GA-EN-13", W, L, ["baseline"], "Generated example without evidence is unsupported.",
    {"pattern": "be used to + V-ing", "use_key": "accustomed", "meaning": "be accustomed to",
     "formation": "be used to + V-ing", "recognition_prompt": RP,
     "examples": [ex("I am used to waking up early.", "Waking early is normal for me.", "generated")]},
    blocked("GENERATED_EXAMPLE_EVIDENCE_REQUIRED"))
add("GA-EN-14", W, L, ["multi_pattern"], "Third conditional keeps both clauses in one pattern.",
    {"pattern": "if + past perfect, would have + V3", "use_key": "unreal-past", "meaning": "imagined past result",
     "formation": "if + had + V3, would have + V3", "recognition_prompt": RP,
     "examples": [ex("If I had studied, I would have passed.", "I did not study and did not pass.")]},
    ready())
add("GA-EN-15", W, L, ["baseline"], "Missing formation blocks; nothing is generated.",
    {"pattern": "passive voice", "use_key": "passive", "meaning": "the subject receives the action",
     "formation": "", "recognition_prompt": RP,
     "examples": [ex("The letter was written by Mai.", "Mai wrote the letter.")]},
    blocked("REQUIRED_CONTENT"))

# ---------------------------------------------------------------- vocab_revamp
W = "vocab_revamp"
H = "SOURCE_NATIVE_HISTORY_REVIEW"
PNG = {"png": [2, 2]}
MP3 = {"fixture": "audio/tone.mp3"}
JV = "Japanese Vocab (legacy)"
JV_FIELDS = lambda e, r, m, a="", p="", n="": [["Expression", e], ["Reading", r], ["Meaning", m],
                                              ["Audio", a], ["Picture", p], ["Notes", n]]
JV_MAP = {"expression": "Expression", "reading": "Reading", "meaning": "Meaning"}
L = "ja"
revamp("VR-JA-01", W, L, ["kanji", "baseline"], "Plain mapped fields become source-backed candidates.",
       JV, JV_FIELDS("食べる", "たべる", "to eat"), JV_MAP,
       blocked(H, staged={"expression": "食べる", "reading": "たべる", "meaning": "to eat"}))
revamp("VR-JA-02", W, L, ["kana"], "Kana-only note.", JV, JV_FIELDS("ありがとう", "ありがとう", "thank you"), JV_MAP,
       blocked(H, staged={"expression": "ありがとう"}))
revamp("VR-JA-03", W, L, ["kanji", "homograph"], "Homograph note keeps its own meaning; no sense is guessed.",
       JV, JV_FIELDS("橋", "はし", "bridge"), JV_MAP,
       blocked(H, staged={"meaning": "bridge"}, absent=["DICTIONARY_SENSE_REVIEW"]))
revamp("VR-JA-04", W, L, ["kanji"], "Unmapped nonempty Notes field is reviewed, not dropped.",
       JV, JV_FIELDS("走る", "はしる", "to run", n="JLPT N5"), JV_MAP,
       blocked(H, "SOURCE_UNMAPPED_FIELD_REVIEW"))
revamp("VR-JA-05", W, L, ["kanji", "adversarial"], "Formatting HTML is reduced to text and reviewed.",
       JV, JV_FIELDS("見る", "みる", "<b>to see</b>; <i>to look</i>"), JV_MAP,
       blocked(H, "SOURCE_HTML_TEXT_REVIEW", staged={"meaning": "to see; to look"}))
revamp("VR-JA-06", W, L, ["kanji", "adversarial"], "Script content never reaches a staged field.",
       JV, JV_FIELDS("聞く", "きく", "<script>alert(1)</script>to listen"), JV_MAP,
       blocked(H, "SOURCE_HTML_TEXT_REVIEW", staged={"meaning": "to listen"}, staged_excludes={"meaning": "alert"}))
revamp("VR-JA-07", W, L, ["kanji", "mixed_image"], "Picture role with verified image bytes stays archived.",
       JV, JV_FIELDS("犬", "いぬ", "dog", p='<img src="inu.png">'),
       {**JV_MAP, "picture": "Picture"},
       blocked(H, "SOURCE_MEDIA_CONTENT_REVIEW", media_archived=["inu.png"]),
       media={"inu.png": PNG})
revamp("VR-JA-08", W, L, ["kanji", "mixed_image"], "A referenced picture that cannot be read needs review.",
       JV, JV_FIELDS("猫", "ねこ", "cat", p='<img src="neko.jpg">'), {**JV_MAP, "picture": "Picture"},
       blocked(H, "SOURCE_MEDIA_MISSING_REVIEW"), media={"neko.jpg": None})
revamp("VR-JA-09", W, L, ["kanji", "mixed_image"], "Corrupt image bytes are kept but flagged.",
       JV, JV_FIELDS("鳥", "とり", "bird", p='<img src="tori.png">'), {**JV_MAP, "picture": "Picture"},
       blocked(H, "SOURCE_MEDIA_FORMAT_REVIEW", media_archived=["tori.png"]), media={"tori.png": {"text": "not a png"}})
revamp("VR-JA-10", W, L, ["kanji", "shared_media"], "One file referenced by two fields is archived once.",
       JV, JV_FIELDS("山", "やま", "mountain", a="[sound:yama.mp3]", n="[sound:yama.mp3]"),
       {**JV_MAP, "audio": "Audio"},
       blocked(H, "SOURCE_MEDIA_CONTENT_REVIEW", media_archived=["yama.mp3"]), media={"yama.mp3": MP3})
revamp("VR-JA-11", W, L, ["kanji", "vietnamese_explanation"], "Vietnamese meaning from the source is kept.",
       JV, JV_FIELDS("勉強", "べんきょう", "học tập"), JV_MAP,
       blocked(H, staged={"meaning": "học tập"}), settings={"learning.explanation_language": "vi"})
revamp("VR-JA-12", W, L, ["kanji", "mixed_model"], "Basic model: Front/Back mapping.",
       "Basic", [["Front", "時間"], ["Back", "time"]], {"expression": "Front", "meaning": "Back"},
       blocked(H, staged={"expression": "時間", "meaning": "time"}))
revamp("VR-JA-13", W, L, ["kanji", "mixed_model"], "Ruby markup is rich content and is not flattened.",
       "Basic", [["Front", "<ruby>漢字<rt>かんじ</rt></ruby>"], ["Back", "kanji"]],
       {"expression": "Front", "meaning": "Back"}, blocked(H, "SOURCE_RICH_FIELD_REVIEW"))
revamp("VR-JA-14", W, L, ["kanji", "task_leakage"], "A production flag is a task candidate needing review.",
       "Japanese Vocab (production)", [["Expression", "書く"], ["Meaning", "to write"], ["Produce", "y"]],
       {"expression": "Expression", "meaning": "Meaning", "enable_production": "Produce"},
       blocked(H, "SOURCE_TASK_MAPPING_REVIEW"))
revamp("VR-JA-15", W, L, ["kanji"], "Language field disagreeing with the purpose is a conflict.",
       "Japanese Vocab (lang)", [["Expression", "雨"], ["Meaning", "rain"], ["Lang", "en"]],
       {"expression": "Expression", "meaning": "Meaning", "language": "Lang"},
       blocked(H, "SOURCE_LANGUAGE_CONFLICT"))

EV = "English Vocab (legacy)"
EV_FIELDS = lambda w, d, e="", ipa="", img="": [["Word", w], ["Definition", d], ["Example", e], ["IPA", ipa], ["Image", img]]
EV_MAP = {"expression": "Word", "meaning": "Definition"}
L = "en"
revamp("VR-EN-01", W, L, ["baseline"], "Plain English note.", EV, EV_FIELDS("run", "to move fast"), EV_MAP,
       blocked(H, staged={"expression": "run", "meaning": "to move fast"}))
revamp("VR-EN-02", W, L, ["homograph"], "bank (river) source meaning is not overridden.",
       EV, EV_FIELDS("bank", "land along a river"), EV_MAP, blocked(H, staged={"meaning": "land along a river"}))
revamp("VR-EN-03", W, L, ["homograph"], "lead (metal) keeps IPA as pronunciation.",
       EV, EV_FIELDS("lead", "a soft metal", ipa="/lɛd/"), {**EV_MAP, "pronunciation": "IPA"},
       blocked(H, staged={"pronunciation": "/lɛd/"}))
revamp("VR-EN-04", W, L, ["baseline"], "Structured example pairs become source-backed candidates for review.",
       EV, EV_FIELDS("borrow", "take for a time",
                     e='[{"sentence":"Can I borrow a pen?","translation":""}]'), {**EV_MAP, "examples": "Example"},
       blocked(H, "SOURCE_EXAMPLES_REVIEW", absent=["SOURCE_EXAMPLES_SCHEMA_REVIEW"]))
revamp("VR-EN-05", W, L, ["baseline"], "Unmapped example text is reviewed rather than lost.",
       EV, EV_FIELDS("borrow", "take for a time", e="Can I borrow a pen?"), EV_MAP,
       blocked(H, "SOURCE_UNMAPPED_FIELD_REVIEW"))
revamp("VR-EN-06", W, L, ["adversarial"], "Event-handler HTML is stripped to text.",
       EV, EV_FIELDS("click", '<span onclick="steal()">to press a button</span>'), EV_MAP,
       blocked(H, "SOURCE_HTML_TEXT_REVIEW", staged={"meaning": "to press a button"}, staged_excludes={"meaning": "steal"}))
revamp("VR-EN-07", W, L, ["adversarial"], "Encoded markup becomes literal text, never live HTML.",
       EV, EV_FIELDS("tag", "&lt;b&gt;label&lt;/b&gt;"), EV_MAP,
       blocked(H, "SOURCE_HTML_TEXT_REVIEW", staged={"meaning": "<b>label</b>"}))
revamp("VR-EN-08", W, L, ["mixed_image"], "Image with verified bytes.",
       EV, EV_FIELDS("apple", "a round fruit", img='<img src="apple.png">'), {**EV_MAP, "picture": "Image"},
       blocked(H, "SOURCE_MEDIA_CONTENT_REVIEW", media_archived=["apple.png"]),
       media={"apple.png": PNG})
revamp("VR-EN-09", W, L, ["mixed_image"], "Missing image bytes need review.",
       EV, EV_FIELDS("pear", "a fruit", img='<img src="pear.png">'), {**EV_MAP, "picture": "Image"},
       blocked(H, "SOURCE_MEDIA_MISSING_REVIEW"), media={"pear.png": None})
revamp("VR-EN-10", W, L, ["shared_media"], "Same picture in two fields is one archived asset.",
       EV, EV_FIELDS("tree", '<img src="tree.png"> a tall plant', img='<img src="tree.png">'),
       {**EV_MAP, "picture": "Image"},
       blocked(H, "SOURCE_HTML_TEXT_REVIEW", media_archived=["tree.png"]), media={"tree.png": PNG})
revamp("VR-EN-11", W, L, ["vietnamese_explanation"], "Vietnamese definition kept from source.",
       EV, EV_FIELDS("necessary", "cần thiết"), EV_MAP, blocked(H, staged={"meaning": "cần thiết"}),
       settings={"learning.explanation_language": "vi"})
revamp("VR-EN-12", W, L, ["mixed_model"], "Basic (and reversed card) model mapped explicitly.",
       "Basic (and reversed card)", [["Front", "quiet"], ["Back", "making little noise"]],
       {"expression": "Front", "meaning": "Back"}, blocked(H, staged={"expression": "quiet"}), reps=7)
revamp("VR-EN-13", W, L, ["mixed_model"], "One field mapped to two roles is a combined field.",
       "Basic", [["Front", "dog"], ["Back", "an animal"]],
       {"expression": "Front", "meaning": "Back", "usage": "Back"}, blocked(H, "SOURCE_COMBINED_FIELD_REVIEW"))
revamp("VR-EN-14", W, L, ["task_leakage"], "Spelling flag is only a candidate.",
       "English Vocab (spelling)", [["Word", "necessary"], ["Definition", "needed"], ["Spell", "1"]],
       {"expression": "Word", "meaning": "Definition", "enable_spelling": "Spell"},
       blocked(H, "SOURCE_TASK_MAPPING_REVIEW"))
revamp("VR-EN-15", W, L, ["adversarial"], "Template syntax in a source field is rich content.",
       EV, EV_FIELDS("brace", "{{Definition}} a support"), EV_MAP, blocked(H, "SOURCE_RICH_FIELD_REVIEW"))

# ---------------------------------------------------------------- grammar_revamp
W = "grammar_revamp"
JG = "Japanese Grammar (legacy)"
JG_FIELDS = lambda p, m, f, e="", img="": [["Pattern", p], ["Meaning", m], ["Formation", f], ["Example", e], ["Image", img]]
JG_MAP = {"pattern": "Pattern", "meaning": "Meaning", "formation": "Formation"}
L = "ja"
revamp("GR-JA-01", W, L, ["kana", "vietnamese_explanation", "baseline"], "Vietnamese preset meaning kept.",
       JG, JG_FIELDS("〜てもいい", "được phép", "V-て + もいい"), JG_MAP,
       blocked(H, staged={"pattern": "〜てもいい", "meaning": "được phép", "formation": "V-て + もいい"}))
revamp("GR-JA-02", W, L, ["multi_pattern"], "Two patterns in one note are not split automatically.",
       JG, JG_FIELDS("〜ても<br>〜てもいい", "dù ...<br>được phép", "V-て + も"), JG_MAP,
       blocked(H, "SOURCE_HTML_TEXT_REVIEW", staged={"pattern": "〜ても\n〜てもいい"}, documents=1))
revamp("GR-JA-03", W, L, ["kana"], "Plain-text examples are never split automatically.",
       JG, JG_FIELDS("〜ながら", "vừa ... vừa", "V-ます + ながら", e="歩きながら話す。"),
       {**JG_MAP, "examples": "Example"}, blocked(H, "SOURCE_EXAMPLES_SCHEMA_REVIEW", "GRAMMAR_EXAMPLE_REQUIRED"))
revamp("GR-JA-04", W, L, ["mixed_image"], "Grammar card image is archived with its bytes.",
       JG, JG_FIELDS("〜たら", "nếu", "V-た + ら", img='<img src="tara.png">'),
       {**JG_MAP, "picture": "Image"},
       blocked(H, "SOURCE_MEDIA_CONTENT_REVIEW", media_archived=["tara.png"]),
       media={"tara.png": PNG})
revamp("GR-JA-05", W, L, ["mixed_image"], "Missing grammar image needs review.",
       JG, JG_FIELDS("〜ば", "nếu", "V-ば", img='<img src="ba.png">'), {**JG_MAP, "picture": "Image"},
       blocked(H, "SOURCE_MEDIA_MISSING_REVIEW"), media={"ba.png": None})
revamp("GR-JA-06", W, L, ["adversarial"], "Script in the formation never reaches staged text.",
       JG, JG_FIELDS("〜のに", "mặc dù", "<script>x()</script>普通形 + のに"), JG_MAP,
       blocked(H, "SOURCE_HTML_TEXT_REVIEW", staged={"formation": "普通形 + のに"}, staged_excludes={"formation": "x()"}))
revamp("GR-JA-07", W, L, ["homograph", "kana"], "ように with a source use key.",
       "Japanese Grammar (use key)", [["Pattern", "〜ように"], ["Use", "purpose"], ["Meaning", "để"],
                                      ["Formation", "V + ように"]],
       {"pattern": "Pattern", "use_key": "Use", "meaning": "Meaning", "formation": "Formation"},
       blocked(H, staged={"use_key": "purpose"}))
revamp("GR-JA-08", W, L, ["kanji"], "Unmapped notes are reviewed, not dropped.",
       "Japanese Grammar (notes)", [["Pattern", "〜なければならない"], ["Meaning", "phải"],
                                    ["Formation", "V-なければ + ならない"], ["Notes", "N4"]],
       {"pattern": "Pattern", "meaning": "Meaning", "formation": "Formation"},
       blocked(H, "SOURCE_UNMAPPED_FIELD_REVIEW"))
revamp("GR-JA-09", W, L, ["task_leakage"], "Application flag is a task candidate.",
       "Japanese Grammar (exercise)", [["Pattern", "〜ながら"], ["Meaning", "vừa"], ["Formation", "V-ます + ながら"],
                                       ["Exercise", "テレビを見＿＿食べる"], ["Answer", "ながら"], ["Apply", "y"]],
       {"pattern": "Pattern", "meaning": "Meaning", "formation": "Formation", "exercise_prompt": "Exercise",
        "exercise_answer": "Answer", "enable_application": "Apply"},
       blocked(H, "SOURCE_TASK_MAPPING_REVIEW"))
revamp("GR-JA-10", W, L, ["shared_media", "kana"], "Shared audio referenced twice is archived once.",
       "Japanese Grammar (audio)", [["Pattern", "〜けど"], ["Meaning", "nhưng"], ["Formation", "普通形 + けど"],
                                    ["Audio", "[sound:kedo.mp3]"], ["Example", "高いけど買う。[sound:kedo.mp3]"]],
       {"pattern": "Pattern", "meaning": "Meaning", "formation": "Formation", "audio": "Audio"},
       blocked(H, "SOURCE_MEDIA_CONTENT_REVIEW", media_archived=["kedo.mp3"]), media={"kedo.mp3": MP3})
revamp("GR-JA-11", W, L, ["mixed_model"], "Basic grammar card with Front/Back.",
       "Basic", [["Front", "〜すぎる"], ["Back", "quá"]], {"pattern": "Front", "meaning": "Back"},
       blocked(H, staged={"pattern": "〜すぎる"}))
revamp("GR-JA-12", W, L, ["mixed_model", "multi_pattern"], "Back with list markup keeps every item.",
       "Basic", [["Front", "〜ばかり"], ["Back", "<ul><li>chỉ toàn</li><li>vừa mới</li></ul>"]],
       {"pattern": "Front", "meaning": "Back"}, blocked(H, "SOURCE_HTML_TEXT_REVIEW", staged_contains={"meaning": "vừa mới"}))
revamp("GR-JA-13", W, L, ["kanji"], "Explanation language field matching the preset is fine.",
       "Japanese Grammar (lang)", [["Pattern", "〜はずだ"], ["Meaning", "chắc là"], ["Formation", "普通形 + はずだ"],
                                   ["Expl", "vi"]],
       {"pattern": "Pattern", "meaning": "Meaning", "formation": "Formation", "explanation_language": "Expl"},
       blocked(H, absent=["SOURCE_LANGUAGE_CONFLICT"]))
revamp("GR-JA-14", W, L, ["kanji"], "Explanation language field disagreeing with the preset conflicts.",
       "Japanese Grammar (lang)", [["Pattern", "〜はずだ"], ["Meaning", "should be"], ["Formation", "普通形 + はずだ"],
                                   ["Expl", "en"]],
       {"pattern": "Pattern", "meaning": "Meaning", "formation": "Formation", "explanation_language": "Expl"},
       blocked(H, "SOURCE_LANGUAGE_CONFLICT"))
revamp("GR-JA-15", W, L, ["adversarial"], "Template syntax in the pattern is rich content.",
       JG, JG_FIELDS("{{Pattern}}〜まで", "cho đến", "N + まで"), JG_MAP, blocked(H, "SOURCE_RICH_FIELD_REVIEW"))

EG = "English Grammar (legacy)"
EG_FIELDS = lambda p, m, f, e="", img="": [["Structure", p], ["Meaning", m], ["Form", f], ["Example", e], ["Image", img]]
EG_MAP = {"pattern": "Structure", "meaning": "Meaning", "formation": "Form"}
L = "en"
revamp("GR-EN-01", W, L, ["baseline"], "Plain English grammar note.",
       EG, EG_FIELDS("used to + V", "past habit", "used to + base verb"), EG_MAP,
       blocked(H, staged={"pattern": "used to + V", "formation": "used to + base verb"}))
revamp("GR-EN-02", W, L, ["multi_pattern"], "Several tenses in one note stay one reviewed draft.",
       EG, EG_FIELDS("present perfect<br>past simple", "experience vs finished time", "have + V3<br>V2"), EG_MAP,
       blocked(H, "SOURCE_HTML_TEXT_REVIEW", documents=1, staged={"pattern": "present perfect\npast simple"}))
revamp("GR-EN-03", W, L, ["baseline"], "Plain-text example sentences need review, not guessing.",
       EG, EG_FIELDS("had better", "strong advice", "had better + V", e="You had better go."),
       {**EG_MAP, "examples": "Example"}, blocked(H, "SOURCE_EXAMPLES_SCHEMA_REVIEW", "GRAMMAR_EXAMPLE_REQUIRED"))
revamp("GR-EN-04", W, L, ["vietnamese_explanation"], "Vietnamese meaning with explicit override.",
       EG, EG_FIELDS("be going to", "dự định", "be going to + V"), EG_MAP,
       blocked(H, staged={"meaning": "dự định"}), settings={"learning.explanation_language": "vi"})
revamp("GR-EN-05", W, L, ["mixed_image"], "Grammar chart image with bytes.",
       EG, EG_FIELDS("conditionals", "if-clauses", "if + present, will + V", img='<img src="cond.png">'),
       {**EG_MAP, "picture": "Image"},
       blocked(H, "SOURCE_MEDIA_CONTENT_REVIEW", media_archived=["cond.png"]), media={"cond.png": PNG})
revamp("GR-EN-06", W, L, ["mixed_image"], "Unreadable chart image bytes are flagged.",
       EG, EG_FIELDS("passive", "receive the action", "be + V3", img='<img src="passive.gif">'),
       {**EG_MAP, "picture": "Image"},
       blocked(H, "SOURCE_MEDIA_FORMAT_REVIEW", media_archived=["passive.gif"]), media={"passive.gif": {"text": "GIF89a?"}})
revamp("GR-EN-07", W, L, ["adversarial"], "iframe markup is stripped to text.",
       EG, EG_FIELDS("so ... that", '<iframe src="https://example.invalid"></iframe>result', "so + adj + that"), EG_MAP,
       blocked(H, "SOURCE_HTML_TEXT_REVIEW", staged={"meaning": "result"}, staged_excludes={"meaning": "iframe"}))
revamp("GR-EN-08", W, L, ["homograph"], "would (habit) with a use key from the source.",
       "English Grammar (use key)", [["Structure", "would + V"], ["Use", "past-habit"], ["Meaning", "repeated past action"],
                                     ["Form", "would + V"]],
       {"pattern": "Structure", "use_key": "Use", "meaning": "Meaning", "formation": "Form"},
       blocked(H, staged={"use_key": "past-habit"}))
revamp("GR-EN-09", W, L, ["homograph"], "would (conditional) with a different use key.",
       "English Grammar (use key)", [["Structure", "would + V"], ["Use", "conditional"], ["Meaning", "unreal result"],
                                     ["Form", "would + V"]],
       {"pattern": "Structure", "use_key": "Use", "meaning": "Meaning", "formation": "Form"},
       blocked(H, staged={"use_key": "conditional"}))
revamp("GR-EN-10", W, L, ["task_leakage"], "An exercise flag is a candidate only.",
       "English Grammar (exercise)", [["Structure", "used to"], ["Meaning", "past habit"], ["Form", "used to + V"],
                                      ["Gap", "I ___ swim."], ["Answer", "used to"], ["Apply", "yes"]],
       {"pattern": "Structure", "meaning": "Meaning", "formation": "Form", "exercise_prompt": "Gap",
        "exercise_answer": "Answer", "enable_application": "Apply"},
       blocked(H, "SOURCE_TASK_MAPPING_REVIEW"))
revamp("GR-EN-11", W, L, ["mixed_model"], "Cloze-style model with an unmapped Text field.",
       "Cloze", [["Text", "I {{c1::used to}} swim."], ["Back Extra", "past habit"]],
       {"pattern": "Back Extra"}, blocked(H, "SOURCE_UNMAPPED_FIELD_REVIEW"))
revamp("GR-EN-12", W, L, ["mixed_model"], "Basic grammar card.",
       "Basic", [["Front", "had better"], ["Back", "strong advice"]], {"pattern": "Front", "meaning": "Back"},
       blocked(H, staged={"meaning": "strong advice"}))
revamp("GR-EN-13", W, L, ["shared_media"], "Audio used in two fields is archived once.",
       "English Grammar (audio)", [["Structure", "would rather"], ["Meaning", "prefer"], ["Form", "would rather + V"],
                                   ["Audio", "[sound:rather.ogg]"], ["Example", "I'd rather stay. [sound:rather.ogg]"]],
       {"pattern": "Structure", "meaning": "Meaning", "formation": "Form", "audio": "Audio"},
       blocked(H, "SOURCE_MEDIA_CONTENT_REVIEW", media_archived=["rather.ogg"]),
       media={"rather.ogg": {"fixture": "audio/tone.ogg"}})
revamp("GR-EN-14", W, L, ["baseline"], "Language field matching the purpose is accepted.",
       "English Grammar (lang)", [["Structure", "had better"], ["Meaning", "advice"], ["Form", "had better + V"],
                                  ["Lang", "en"]],
       {"pattern": "Structure", "meaning": "Meaning", "formation": "Form", "language": "Lang"},
       blocked(H, absent=["SOURCE_LANGUAGE_CONFLICT"]))
revamp("GR-EN-15", W, L, ["adversarial"], "Control of data URIs: image src with javascript is not media.",
       EG, EG_FIELDS("passive", "be + V3", "be + V3", img='<img src="javascript:alert(1)">'),
       {**EG_MAP}, blocked(H, "SOURCE_UNMAPPED_FIELD_REVIEW"))


def main():
    ids = [f["id"] for f in FIXTURES]
    assert len(ids) == len(set(ids)) == 120, len(ids)
    text = json.dumps({"schema_version": 1, "fixtures": FIXTURES}, ensure_ascii=False, indent=1) + "\n"
    if "--check" in sys.argv:
        if OUT.read_text(encoding="utf-8") != text:
            raise SystemExit(f"{OUT} is stale; run scripts/generate-semantic-corpus.py")
        return
    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text(text, encoding="utf-8")
    print(f"wrote {len(FIXTURES)} fixtures to {OUT.relative_to(ROOT)}")


if __name__ == "__main__":
    main()
