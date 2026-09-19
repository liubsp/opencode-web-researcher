use research_core::ChatReference;

#[test]
fn accepts_chat_ids_and_project_urls_but_not_other_destinations() {
    let id = "00000000-0000-4000-8000-000000000001";
    assert_eq!(
        ChatReference::parse(id).unwrap().url,
        format!("https://chatgpt.com/c/{id}")
    );
    let project = format!("https://chatgpt.com/g/g-p-example/c/{id}");
    assert_eq!(
        ChatReference::parse(&format!("{project}?ref=test#fragment"))
            .unwrap()
            .url,
        project
    );
    for bad in [
        "https://evil.org/c/00000000-0000-4000-8000-000000000001",
        "https://user@chatgpt.com/c/00000000-0000-4000-8000-000000000001",
        "https://chatgpt.com/share/00000000-0000-4000-8000-000000000001",
        "https://chatgpt.com/g/g-p-example/project",
        "file:///c/example",
        "not-a-chat",
        "https://chatgpt.com/c/../../other",
    ] {
        assert!(ChatReference::parse(bad).is_err(), "{bad}");
    }
}
