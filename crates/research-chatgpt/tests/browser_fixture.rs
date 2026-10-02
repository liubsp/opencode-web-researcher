use anyhow::{Context, Result};
use research_browser::Chrome;
use research_core::Config;
use serde_json::json;

enum Fixture {
    Owned(tempfile::TempDir),
    Shared(std::path::PathBuf),
}

impl std::ops::Deref for Fixture {
    type Target = std::path::Path;
    fn deref(&self) -> &Self::Target {
        match self {
            Self::Owned(dir) => dir.path(),
            Self::Shared(dir) => dir,
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if matches!(self, Self::Shared(_)) {
            return;
        }
        // Close only this test's isolated browser, including after a failed assertion.
        std::thread::scope(|scope| {
            let _ = scope
                .spawn(|| -> Result<()> {
                    let activity = self.join("chrome-activity");
                    if !activity.exists() {
                        return Ok(());
                    }
                    std::fs::File::options()
                        .write(true)
                        .open(activity)?
                        .set_modified(
                            std::time::SystemTime::now() - std::time::Duration::from_secs(3600),
                        )?;
                    tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()?
                        .block_on(Chrome::close_if_idle(self, &Config::default()))?;
                    Ok(())
                })
                .join();
        });
    }
}

fn fixture_dir() -> Result<Fixture> {
    if let Some(path) = std::env::var_os("WEB_RESEARCH_FIXTURE_HOME") {
        let path = std::path::PathBuf::from(path);
        anyhow::ensure!(
            path.join("fixture-owned").is_file(),
            "Fixture directory is not isolated; use scripts/check-browser.mjs"
        );
        return Ok(Fixture::Shared(path));
    }
    Ok(Fixture::Owned(
        tempfile::Builder::new()
            .prefix("research-fixture-")
            .tempdir()?,
    ))
}

#[tokio::test]
#[ignore = "Requires installed Chrome; isolated text round-trips, no account message"]
async fn canonical_line_endings_preserve_zero_width_content() -> Result<()> {
    let dir = fixture_dir()?;
    let chrome = Chrome::ensure(&dir, &Config::default()).await?;
    let mut page = chrome.open("about:blank").await?;
    for prompt in [
        "one\r\n\r\ntwo",
        "one\r\rtwo",
        "one\u{200b}two",
        "one\u{feff}two",
    ] {
        page.eval("document.body.innerHTML='<form><div id=\"prompt-textarea\" contenteditable=\"true\"><p><br class=\"ProseMirror-trailingBreak\"></p></div></form>'").await?;
        let mut receipt = None;
        let result = research_chatgpt::fill_managed(&mut page, prompt, |draft| {
            receipt = Some(draft.to_owned());
            Ok(())
        })
        .await;
        let diagnostic = page.eval("({url:location.href,html:document.body.innerHTML,active:document.activeElement?.tagName,selection:window.getSelection()?.anchorNode?.parentElement?.outerHTML})").await?;
        result.with_context(|| {
            format!(
                "round-trip expected {:?}, received {:?}, fixture: {diagnostic}",
                research_core::canonical_prompt(prompt),
                receipt
            )
        })?;
        let canonical = research_core::canonical_prompt(prompt);
        assert_eq!(
            research_chatgpt::inspect(&mut page).await?["composer_text"],
            canonical
        );
        assert_eq!(receipt.as_deref(), Some(canonical.as_str()));
    }
    chrome.close(&page.id).await?;
    Ok(())
}

#[tokio::test]
#[ignore = "Requires installed Chrome; isolated mixed markup and literal whitespace"]
async fn active_mixed_turn_markup_preserves_literal_blank_lines() -> Result<()> {
    let dir = fixture_dir()?;
    let chrome = Chrome::ensure(&dir, &Config::default()).await?;
    let mut page = chrome.open("about:blank").await?;
    let html = r#"<div hidden><div data-message-author-role="user">hidden old prompt</div></div>
      <main><div data-message-author-role="user" data-message-id="legacy">visible old prompt</div>
      <div data-content-search-unit-key="new:user" data-chatgpt-search-message-ids="current"><h4 aria-hidden="true">You said:</h4><div data-user-message-bubble>current prompt</div></div>
      <div data-content-search-unit-key="new:assistant"><div data-markdown-text-style="assistant-message"><pre><code>first


last</code></pre></div></div></main>"#;
    page.eval(&format!("document.body.innerHTML={}", json!(html)))
        .await?;
    let snapshot = research_chatgpt::inspect(&mut page).await?;
    assert_eq!(snapshot["turns"].as_array().unwrap().len(), 3);
    assert_eq!(snapshot["turns"][1]["text"], "current prompt");
    assert!(
        snapshot["turns"][2]["markdown"]
            .as_str()
            .unwrap()
            .contains("first\n\n\nlast")
    );
    assert!(research_chatgpt::observation::user_turn_confirmed(
        &snapshot,
        1,
        "current prompt"
    ));
    chrome.close(&page.id).await?;
    Ok(())
}

#[tokio::test]
#[ignore = "Requires installed Chrome; isolated accessible Stop control"]
async fn stop_uses_the_current_accessible_control_not_only_legacy_ids() -> Result<()> {
    let dir = fixture_dir()?;
    let chrome = Chrome::ensure(&dir, &Config::default()).await?;
    let mut page = chrome.open("about:blank").await?;
    page.eval("document.body.innerHTML='<nav><button aria-label=\"Stop\" onclick=\"window.wrong=true\"></button></nav><form><button aria-label=\"Stop\" onclick=\"window.stopped=true;this.remove()\"></button></form>'").await?;
    assert_eq!(research_chatgpt::inspect(&mut page).await?["busy"], true);
    assert!(research_chatgpt::stop(&mut page).await?);
    assert_eq!(
        page.eval("window.stopped === true && window.wrong === undefined")
            .await?,
        true
    );
    assert!(!research_chatgpt::stop(&mut page).await?);
    assert_eq!(research_chatgpt::inspect(&mut page).await?["busy"], false);
    chrome.close(&page.id).await?;
    Ok(())
}

#[tokio::test]
#[ignore = "Requires installed Chrome; isolated delayed explicit model selection"]
async fn explicit_model_waits_for_readiness_and_verifies_selection() -> Result<()> {
    let dir = fixture_dir()?;
    let chrome = Chrome::ensure(&dir, &Config::default()).await?;
    let mut page = chrome.open("about:blank").await?;
    let html = r#"<form><div id="prompt-textarea" contenteditable="true"><p></p></div><button type="button" id="trigger" aria-label="Select ChatGPT model" disabled>Old model</button></form>
      <div id="picker" data-testid="composer-intelligence-picker-content" hidden><span id="announcement">Extra High, 5 of 5.</span>
      <div role="menuitem" aria-label="Power" aria-describedby="announcement"><span role="slider" aria-valuenow="4" aria-valuemax="4"></span></div>
      <button id="wanted" role="menuitemradio" aria-checked="false">Wanted model</button></div>"#;
    page.eval(&format!("document.body.innerHTML={}", json!(html)))
        .await?;
    page.eval(r#"(() => {
      const trigger=document.getElementById('trigger'), picker=document.getElementById('picker');
      setTimeout(()=>{trigger.disabled=false;trigger.setAttribute('aria-haspopup','menu')},600);
      trigger.onclick=()=>{window.openings=(window.openings||0)+1;trigger.setAttribute('aria-expanded','true');setTimeout(()=>{picker.hidden=false},600)};
      document.getElementById('wanted').onclick=()=>{setTimeout(()=>{document.getElementById('wanted').setAttribute('aria-checked','true');trigger.textContent='Wanted model'},500)};
      document.addEventListener('keydown',e=>{if(e.key==='Escape'){picker.hidden=true;trigger.setAttribute('aria-expanded','false')}});
    })()"#).await?;
    let config = Config {
        model: "Wanted model".into(),
        ..Config::default()
    };
    let result = research_chatgpt::prepare(&mut page, &config, false, true).await;
    let diagnostic = page.eval("({url:location.href,openings:window.openings,hidden:document.getElementById('picker')?.hidden,trigger:document.getElementById('trigger')?.outerHTML,bodyChars:document.body.innerHTML.length})").await?;
    let prepared = result.with_context(|| format!("model fixture state: {diagnostic}"))?;
    assert_eq!(prepared["reasoning"]["selected"], "Extra High");
    assert_eq!(
        page.eval("document.getElementById('wanted').getAttribute('aria-checked')")
            .await?,
        "true"
    );
    page.eval("(() => {const trigger=document.getElementById('trigger'), open=trigger.onclick;trigger.onclick=()=>{open();document.getElementById('prompt-textarea').innerHTML='<p><span contenteditable=\"false\" data-system-hint-type=\"deep_research\">Deep research</span></p>'}})()").await?;
    assert!(
        research_chatgpt::prepare(&mut page, &config, false, true)
            .await
            .unwrap_err()
            .to_string()
            .contains("mode changed during preparation")
    );
    chrome.close(&page.id).await?;
    Ok(())
}

#[tokio::test]
#[ignore = "Requires installed Chrome; isolated conflicting model control"]
async fn unrelated_checked_radio_cannot_confirm_a_model_selection() -> Result<()> {
    let dir = fixture_dir()?;
    let chrome = Chrome::ensure(&dir, &Config::default()).await?;
    let mut page = chrome.open("about:blank").await?;
    let html = r#"<form><div id="prompt-textarea" contenteditable="true"><p><br></p></div><button type="button" id="trigger" aria-label="Select ChatGPT model" aria-haspopup="menu" onclick="document.getElementById('picker').hidden=false">Old model</button></form><div role="menuitemradio" aria-checked="true">Wanted model</div><div id="picker" data-testid="composer-intelligence-picker-content" hidden><button type="button" role="menuitemradio" aria-checked="true">Old model</button><button type="button" role="menuitemradio" aria-checked="false">Wanted model</button></div>"#;
    page.eval(&format!("document.body.innerHTML={}", json!(html)))
        .await?;
    let config = Config {
        model: "Wanted model".into(),
        ..Config::default()
    };
    assert!(
        research_chatgpt::prepare(&mut page, &config, false, true)
            .await
            .unwrap_err()
            .to_string()
            .contains("could not be verified")
    );
    assert_eq!(
        page.eval("document.getElementById('trigger').innerText")
            .await?,
        "Old model"
    );
    chrome.close(&page.id).await?;
    Ok(())
}

#[tokio::test]
#[ignore = "Requires installed Chrome; isolated delayed mode application"]
async fn deep_mode_requires_applied_selection_not_only_a_click() -> Result<()> {
    let dir = fixture_dir()?;
    let chrome = Chrome::ensure(&dir, &Config::default()).await?;
    let mut page = chrome.open("about:blank").await?;
    let html = r#"<form><div id="prompt-textarea" contenteditable="true"><p><br></p></div><button type="button" aria-label="Add files and more" onclick="document.getElementById('tools').hidden=false">+</button></form><div id="tools" role="menu" hidden><button type="button" id="deep" role="menuitem">Deep research</button></div>"#;
    page.eval(&format!("document.body.innerHTML={}", json!(html)))
        .await?;
    page.eval("document.getElementById('deep').onclick=()=>{document.getElementById('tools').hidden=true;setTimeout(()=>{document.getElementById('prompt-textarea').innerHTML='<p><span contenteditable=\"false\" data-system-hint-type=\"deep_research\">Deep research</span></p>'},600)}").await?;
    let selected = research_chatgpt::prepare(&mut page, &Config::default(), true, true).await?;
    assert_eq!(selected["state"]["mode"], "deep_research");
    assert!(
        research_chatgpt::prepare(&mut page, &Config::default(), false, true)
            .await
            .is_err()
    );
    page.eval(&format!("document.body.innerHTML={}", json!(html)))
        .await?;
    assert!(
        research_chatgpt::prepare(&mut page, &Config::default(), true, true)
            .await
            .unwrap_err()
            .to_string()
            .contains("selection was not applied")
    );
    chrome.close(&page.id).await?;
    Ok(())
}

/// Uses actual Chromium DOM semantics without contacting ChatGPT or sending account messages.
#[tokio::test]
#[ignore = "Requires installed Chrome; synthetic deletion controls only"]
async fn deletion_menu_uses_current_chat_header_without_sidebar_history() -> Result<()> {
    let dir = fixture_dir()?;
    let chrome = Chrome::ensure(&dir, &Config::load(&dir)?).await?;
    let mut page = chrome.open("about:blank").await?;
    let html = r##"<nav><a href="https://chatgpt.com/c/other">Other chat</a>
      <button aria-haspopup="menu" onclick="window.wrong=true">More</button></nav>
      <header><button data-testid="conversation-options-button" aria-label="More"
      onclick="window.opened=(window.opened||0)+1">...</button></header>"##;
    page.eval(&format!("document.body.innerHTML={}", json!(html)))
        .await?;
    let action = include_str!("../src/scripts/action.js")
        .replace("location.href", "'https://chatgpt.com/c/fixture'");
    let wrong =
        format!("({action})({{op:'open_chat_menu',argument:'https://chatgpt.com/c/other'}})");
    assert_eq!(page.eval(&wrong).await?["ok"], false);
    assert_eq!(
        page.eval("window.opened === undefined && window.wrong === undefined")
            .await?,
        true
    );
    let current =
        format!("({action})({{op:'open_chat_menu',argument:'https://chatgpt.com/c/fixture'}})");
    assert_eq!(page.eval(&current).await?["ok"], true);
    assert_eq!(
        page.eval("window.opened === 1 && window.wrong === undefined")
            .await?,
        true
    );
    page.eval("document.querySelector('header').remove()")
        .await?;
    assert_eq!(page.eval(&current).await?["ok"], false);
    assert_eq!(page.eval("window.wrong === undefined").await?, true);
    chrome.close(&page.id).await?;
    Ok(())
}

#[tokio::test]
#[ignore = "Requires installed Chrome; identity-bound accessible deletion toolbar"]
async fn deletion_toolbar_survives_missing_test_ids_and_project_slug_changes() -> Result<()> {
    let dir = fixture_dir()?;
    let chrome = Chrome::ensure(&dir, &Config::load(&dir)?).await?;
    let mut page = chrome.open("about:blank").await?;
    let current = "https://chatgpt.com/g/g-p-0123456789abcdef0123456789abcdef/c/ours";
    let saved = "https://chatgpt.com/g/g-p-0123456789abcdef0123456789abcdef-research/c/ours";
    let action =
        include_str!("../src/scripts/action.js").replace("location.href", "window.fixtureUrl");
    page.eval(&format!("window.fixtureUrl={}", json!(current)))
        .await?;
    page.eval(r#"document.body.innerHTML='<nav><button aria-haspopup="menu" aria-label="More" onclick="window.wrong=true"></button></nav><main><header><button aria-label="Share"></button><button aria-label="Chat options" aria-haspopup="menu" onclick="window.opened=(window.opened||0)+1"></button></header><div data-content-search-unit-key="message:assistant"><button aria-label="More" aria-haspopup="menu" onclick="window.wrong=true"></button></div><form><button aria-label="More" aria-haspopup="menu" onclick="window.wrong=true"></button></form></main>'"#).await?;
    let open = format!(
        "({action})({})",
        json!({"op":"open_chat_menu","argument":saved})
    );
    assert_eq!(page.eval(&open).await?["ok"], true);
    assert_eq!(
        page.eval("window.opened===1 && window.wrong===undefined")
            .await?,
        true
    );
    for wrong in [
        saved.replace("/ours", "/other"),
        saved.replace(
            "0123456789abcdef0123456789abcdef",
            "fedcba9876543210fedcba9876543210",
        ),
        saved.replace("https://chatgpt.com", "https://chatgpt.com.evil.test"),
    ] {
        assert_eq!(
            page.eval(&format!(
                "({action})({})",
                json!({"op":"open_chat_menu","argument":wrong})
            ))
            .await?["ok"],
            false
        );
    }
    page.eval("document.querySelector('header').insertAdjacentHTML('beforeend','<button aria-label=\"Chat options\" aria-haspopup=\"menu\" onclick=\"window.wrong=true\"></button>')").await?;
    assert_eq!(
        page.eval(&open).await?["error"],
        "ambiguous_conversation_menu"
    );
    page.eval("document.querySelector('header').remove()")
        .await?;
    assert_eq!(page.eval(&open).await?["ok"], false);
    assert_eq!(
        page.eval("window.opened===1 && window.wrong===undefined")
            .await?,
        true
    );
    // Header More now opens Plugins, not deletion. Prefer an ID-bound sidebar
    // row even when its canonical link omits the project scope.
    let rows = r#"<nav><div><a href="https://chatgpt.com/c/ours">Owned chat</a><div><button aria-label="Chat actions" aria-haspopup="menu" onclick="window.rowOpened=true;this.setAttribute('aria-expanded','true')"></button></div></div><div><a href="https://chatgpt.com/c/other">Other chat</a><button aria-label="Chat actions" aria-haspopup="menu" onclick="window.wrong=true"></button></div></nav><main><header><button aria-label="Share"></button><button aria-label="More" aria-haspopup="menu" onclick="window.wrong=true"></button></header></main>"#;
    page.eval(&format!("document.body.innerHTML={}", json!(rows)))
        .await?;
    assert_eq!(page.eval(&open).await?["ok"], true);
    assert_eq!(page.eval(&open).await?["ok"], true);
    assert_eq!(
        page.eval("window.rowOpened===true && window.wrong===undefined")
            .await?,
        true
    );
    page.eval("document.querySelector('nav').remove()").await?;
    assert_eq!(page.eval(&open).await?["ok"], false);
    page.eval(r#"document.querySelector('main').insertAdjacentHTML('afterbegin','<a href="https://chatgpt.com/c/ours">Reference</a><button aria-haspopup="menu" aria-label="Project options" onclick="window.wrong=true"></button>')"#).await?;
    assert_eq!(page.eval(&open).await?["ok"], false);
    // Project cards remain available after older chats disappear from sidebar history.
    let landing = current.replace("/c/ours", "/project");
    page.eval(&format!("window.fixtureUrl={}", json!(landing)))
        .await?;
    page.eval(&format!("document.body.innerHTML={}", json!(format!(r#"<main><div><a href="{current}">Owned</a><button id="owned-actions" aria-label="Actions for Owned" aria-controls="owned-menu" aria-haspopup="menu" onclick="this.setAttribute('aria-expanded','true')"></button></div><div><a href="https://chatgpt.com/c/other">Other</a><button aria-haspopup="menu" onclick="window.wrong=true"></button></div></main><div id="owned-menu" role="menu" aria-labelledby="owned-actions"><button role="menuitem" onclick="window.deleted=(window.deleted||0)+1">Delete</button></div><div role="menu"><button role="menuitem" onclick="window.wrong=true">Delete</button></div>"#)))).await?;
    assert_eq!(page.eval(&open).await?["ok"], true);
    let item = format!(
        "({action})({})",
        json!({"op":"delete_menu_item","argument":saved})
    );
    let confirm = format!(
        "({action})({})",
        json!({"op":"confirm_delete","argument":saved})
    );
    assert_eq!(page.eval(&confirm).await?["ok"], false);
    page.eval("document.getElementById('owned-menu').hidden=true")
        .await?;
    assert_eq!(page.eval(&item).await?["ok"], false); // unrelated Delete is never a fallback
    page.eval("document.getElementById('owned-menu').hidden=false;document.getElementById('owned-menu').insertAdjacentHTML('beforeend','<button role=menuitem id=duplicate>Delete</button>')").await?;
    assert_eq!(page.eval(&item).await?["ok"], false);
    page.eval("document.getElementById('duplicate').remove()")
        .await?;
    page.eval(&format!(
        "window.fixtureUrl={}",
        json!(landing.replace(
            "0123456789abcdef0123456789abcdef",
            "fedcba9876543210fedcba9876543210"
        ))
    ))
    .await?;
    assert_eq!(page.eval(&item).await?["ok"], false);
    assert_eq!(page.eval(&open).await?["ok"], false);
    page.eval(&format!("window.fixtureUrl={}", json!(landing)))
        .await?;
    assert_eq!(page.eval(&item).await?["ok"], true);
    assert_eq!(
        page.eval("window.deleted===1 && window.wrong===undefined")
            .await?,
        true
    );
    page.eval("document.body.innerHTML='<div role=dialog>Delete chat?<button onclick=\"window.confirmed=true\">Delete chat</button></div><div role=dialog id=extra>Delete other chat?<button onclick=\"window.wrong=true\">Delete</button></div>'").await?;
    assert_eq!(page.eval(&confirm).await?["ok"], false);
    page.eval("document.getElementById('extra').remove()")
        .await?;
    assert_eq!(
        page.eval(&format!(
            "({action})({})",
            json!({"op":"confirm_delete","argument":saved.replace("/ours", "/other")})
        ))
        .await?["ok"],
        false
    );
    assert_eq!(page.eval(&confirm).await?["ok"], true);
    assert_eq!(page.eval(&confirm).await?["ok"], false); // never confirm twice
    // A listed canonical URL still blocks confirmation for its old friendly slug.
    let deleted_action = include_str!("../src/scripts/action.js");
    page.eval(&format!("document.body.innerHTML={}", json!(format!("<p>Conversation has been deleted. Start a new chat.</p><a href=\"{current}\">Still listed</a>")))).await?;
    let deleted = format!(
        "({deleted_action})({})",
        json!({"op":"deleted","argument":saved})
    );
    assert_eq!(page.eval(&deleted).await?["ok"], false);
    page.eval("document.querySelector('a').href='https://chatgpt.com/c/ours'")
        .await?;
    assert_eq!(page.eval(&deleted).await?["ok"], false);
    page.eval("document.querySelector('a').remove()").await?;
    assert_eq!(page.eval(&deleted).await?["ok"], true);
    page.eval("delete window.__researchCleanup;document.body.innerHTML='<div role=status>Chat deleted</div>'").await?;
    assert_eq!(page.eval(&deleted).await?["ok"], false); // stale/unbound toast is not proof
    page.eval("document.body.innerHTML='<div role=alert>You don’t have access to this conversation.</div>'").await?;
    assert_eq!(page.eval(&deleted).await?["ok"], false);
    assert_eq!(
        page.eval(&format!(
            "({deleted_action})({})",
            json!({"op":"deletion_state","argument":saved})
        ))
        .await?["unavailable"],
        true
    );
    page.eval("document.body.innerHTML='<li data-sonner-toast><div>You don’t have access to this conversation.</div></li>'").await?;
    assert_eq!(
        page.eval(&format!(
            "({deleted_action})({})",
            json!({"op":"deletion_state","argument":saved})
        ))
        .await?["unavailable"],
        true
    );
    chrome.close(&page.id).await?;
    Ok(())
}

#[tokio::test]
#[ignore = "Requires installed Chrome; run explicitly for the browser compatibility check"]
async fn extraction_and_input_against_a_chrome_fixture() -> Result<()> {
    let dir = fixture_dir()?;
    let chrome = Chrome::ensure(&dir, &Config::load(&dir)?).await?;
    let mut page = chrome.open("about:blank").await?;
    assert_eq!(chrome.window_state(&page.id).await?, "minimized");
    let html = r##"<div id="prompt-textarea" contenteditable="true"></div>
      <article><div data-message-author-role="user"><div><span data-inline-selection-pill>Web search</span>check sources pls</div><button data-testid="collapsible-user-message-toggle"><span>Show more</span><span>Show less</span></button></div></article>
      <article><div data-message-author-role="assistant"><h2>Finding</h2><p>See <a href="https://example.org/docs">docs</a></p>
        <pre><code>let answer = 42;</code></pre></div><button data-testid="copy-turn-action-button">Copy</button></article>
      <button id="composer-submit-button" onclick="window.sent=true">Send</button>"##;
    page.eval(&format!("document.body.innerHTML={}", json!(html)))
        .await?;
    let snapshot = research_chatgpt::inspect(&mut page).await?;
    assert_eq!(snapshot["turns"][0]["text"], "check sources pls");
    assert_eq!(snapshot["turns"][1]["complete"], true);
    let markdown = snapshot["turns"][1]["markdown"].as_str().unwrap();
    assert!(markdown.contains("[docs](https://example.org/docs)"));
    assert!(markdown.contains("```\nlet answer = 42;\n```"));
    let read = research_chatgpt::capture_rendered_chat(&mut page).await?;
    assert_eq!(read["turns"].as_array().unwrap().len(), 2);
    assert!(
        read["markdown"]
            .as_str()
            .unwrap()
            .contains("[docs](https://example.org/docs)")
    );
    assert_eq!(page.eval("window.sent === undefined && document.querySelector('#prompt-textarea').textContent === ''").await?, true);
    research_chatgpt::fill(&mut page, "new question pls").await?;
    research_chatgpt::fill(&mut page, "new question pls").await?; // recovery must not duplicate composer content
    assert!(
        research_chatgpt::fill(&mut page, "different question")
            .await
            .is_err()
    );
    research_chatgpt::send(&mut page).await?;
    assert_eq!(page.eval("window.sent").await?, true);
    chrome.close(&page.id).await?;
    Ok(())
}

#[tokio::test]
#[ignore = "Requires installed Chrome; synthetic current composer, no account message"]
async fn current_composer_and_model_picker_fixture() -> Result<()> {
    let dir = fixture_dir()?;
    let chrome = Chrome::ensure(&dir, &Config::load(&dir)?).await?;
    let mut page = chrome.open("about:blank").await?;
    let html = r##"<nav><button aria-label="Send" onclick="window.wrong=true">Send</button></nav>
      <form><div class="ProseMirror" role="textbox" contenteditable="true" aria-label="New chat in Research"><p data-empty-paragraph="true"><br class="ProseMirror-trailingBreak"></p></div>
      <button type="button" aria-label="Select ChatGPT model" aria-haspopup="menu" onclick="document.getElementById('menu').hidden=false">Model</button>
      <button type="button" aria-label="Add files and more" onclick="window.tools=true">+</button>
      <button type="button" aria-label="Send" onclick="window.sent=true">Send</button></form>
      <div id="menu" role="menu" hidden><span id="announcement">Instant, 1 of 5.</span>
      <div role="menuitem" aria-label="Power" aria-describedby="announcement" tabindex="0">
        <span role="slider" aria-valuenow="0" aria-valuemax="4"></span></div>
      <div role="menuitemradio" aria-checked="true">Latest</div></div>"##;
    page.eval(&format!("document.body.innerHTML={}", json!(html)))
        .await?;
    page.eval(r##"(() => {
      const power=document.querySelector('[aria-label="Power"]');
      power.onkeydown=e=>{
        const values=['Instant','Light','Standard','High','Extra High'];
        const slider=power.querySelector('[role="slider"]');
        const n=Math.max(0,Math.min(4,Number(slider.getAttribute('aria-valuenow'))+(e.key==='ArrowRight'?1:-1)));
        setTimeout(()=>{
          slider.setAttribute('aria-valuenow',n);
          document.getElementById('announcement').textContent=values[n]+', '+(n+1)+' of 5.';
        },500);
      };
      document.addEventListener('keydown',e=>{if(e.key==='Escape')document.getElementById('menu').hidden=true});
    })()"##).await?;
    assert_eq!(
        research_chatgpt::inspect(&mut page).await?["composer"],
        true
    );
    let setup = research_chatgpt::prepare(&mut page, &Config::default(), false, true).await?;
    assert_eq!(setup["reasoning"]["selected"], "Extra High");
    research_chatgpt::fill(&mut page, "new question pls").await?;
    research_chatgpt::fill(&mut page, "new question pls").await?;
    assert_eq!(
        research_chatgpt::inspect(&mut page).await?["composer_text"],
        "new question pls"
    );
    research_chatgpt::send(&mut page).await?;
    assert_eq!(
        page.eval("window.sent === true && window.wrong === undefined")
            .await?,
        true
    );
    chrome.close(&page.id).await?;
    Ok(())
}

#[tokio::test]
#[ignore = "Requires installed Chrome; synthetic multiline drafts, no account message"]
async fn multiline_placeholders_and_managed_draft_cleanup() -> Result<()> {
    let dir = fixture_dir()?;
    let chrome = Chrome::ensure(&dir, &Config::load(&dir)?).await?;
    let mut page = chrome.open("about:blank").await?;
    let prompt = "first line\n\nsecond line\nthird line\n\nlast line";
    let paragraphs = r##"<p>first line</p><p><br class="ProseMirror-trailingBreak"></p>
      <p>second line<br>third line</p><p><br class="ProseMirror-trailingBreak"></p><p>last line</p>"##;
    // Whitespace between blocks is not part of ProseMirror's live DOM.
    let paragraphs = paragraphs.replace("\n      ", "");
    let html = format!(
        "<form><div id='prompt-textarea' contenteditable='true'>{paragraphs}</div></form>\
         <div data-message-author-role='user' data-message-id='user-1'>{paragraphs}</div>"
    );
    page.eval(&format!("document.body.innerHTML={}", json!(html)))
        .await?;
    let state = research_chatgpt::inspect(&mut page).await?;
    assert_eq!(state["composer_text"], prompt);
    assert_eq!(state["turns"][0]["text"], prompt);
    research_chatgpt::fill(&mut page, prompt).await?;
    assert!(
        research_chatgpt::clear_managed_draft(&mut page, "unrelated draft")
            .await
            .is_err()
    );
    assert_eq!(
        research_chatgpt::inspect(&mut page).await?["composer_text"],
        prompt
    );
    research_chatgpt::clear_managed_draft(&mut page, prompt).await?;
    assert_eq!(
        research_chatgpt::inspect(&mut page).await?["composer_text"],
        ""
    );
    research_chatgpt::fill(&mut page, "next question\n\nanother paragraph").await?;
    assert_eq!(
        research_chatgpt::inspect(&mut page).await?["composer_text"],
        "next question\n\nanother paragraph"
    );
    page.eval("document.querySelector('#prompt-textarea').innerHTML='<p><span contenteditable=\"false\">Web search</span>private draft</p>'").await?;
    assert!(
        research_chatgpt::clear_managed_draft(&mut page, "private draft")
            .await
            .is_err()
    );
    assert_eq!(
        research_chatgpt::inspect(&mut page).await?["composer_text"],
        "private draft"
    );
    chrome.close(&page.id).await?;
    Ok(())
}

#[tokio::test]
#[ignore = "Requires installed Chrome; synthetic delayed picker, no account message"]
async fn reasoning_waits_for_delayed_controls_without_closing_picker() -> Result<()> {
    let dir = fixture_dir()?;
    let chrome = Chrome::ensure(&dir, &Config::load(&dir)?).await?;
    let mut page = chrome.open("about:blank").await?;
    let html = r##"<form><div id="prompt-textarea" contenteditable="true"><p></p></div>
      <button type="button" id="trigger" aria-label="Select ChatGPT model" disabled>Extra High</button></form>
      <div id="menu" data-testid="composer-intelligence-picker-content" hidden>
        <span id="announcement"></span>
        <div role="menuitem" aria-label="Power" aria-describedby="announcement" tabindex="0">
          <span role="slider" aria-valuenow="4" aria-valuemax="4"></span></div>
      </div>"##;
    page.eval(&format!("document.body.innerHTML={}", json!(html)))
        .await?;
    page.eval(r##"(() => {
      const trigger=document.getElementById('trigger'), menu=document.getElementById('menu');
      setTimeout(()=>{trigger.disabled=false;trigger.setAttribute('aria-haspopup','menu')},600);
      trigger.onclick=()=>{
        window.openings=(window.openings||0)+1;
        const expanded=trigger.getAttribute('aria-expanded')==='true';
        trigger.setAttribute('aria-expanded',String(!expanded));
        if(expanded){menu.hidden=true;return}
        setTimeout(()=>{menu.hidden=false},800);
        setTimeout(()=>{document.getElementById('announcement').textContent='Extra High, 5 of 5.'},1100);
      };
      document.addEventListener('keydown',e=>{if(e.key==='Escape'){menu.hidden=true;trigger.setAttribute('aria-expanded','false')}});
    })()"##).await?;
    let setup = research_chatgpt::prepare(&mut page, &Config::default(), false, true).await?;
    assert_eq!(setup["reasoning"]["selected"], "Extra High");
    assert_eq!(page.eval("window.openings").await?, 1);
    assert_eq!(
        page.eval("document.getElementById('menu').hidden").await?,
        true
    );
    chrome.close(&page.id).await?;
    Ok(())
}

#[tokio::test]
#[ignore = "Requires installed Chrome; synthetic current turn markup, no account message"]
async fn current_turn_markup_confirms_submission_and_response() -> Result<()> {
    let dir = fixture_dir()?;
    let chrome = Chrome::ensure(&dir, &Config::load(&dir)?).await?;
    let mut page = chrome.open("about:blank").await?;
    let prompt = "Find the official source.";
    let html = r##"<main><div class="group"><div><div>
      <div data-chatgpt-search-message-ids="question-1"><div data-content-search-unit-key="fallback-turn-0:0:user"><div data-user-message-bubble="true">Find the official source.</div>
      <button aria-label="Copy message">Copy</button></div></div>
      <div><div data-content-search-unit-key="fallback-turn-0:2:assistant" data-chatgpt-search-message-ids="reply-1">
      <h4>ChatGPT said:</h4><div data-markdown-text-style="assistant-message"><p>See <a href="https://example.org/docs">docs</a>.</p></div>
      </div></div></div><button aria-label="Copy">Copy</button></div></main>"##;
    page.eval(&format!("document.body.innerHTML={}", json!(html)))
        .await?;
    let state = research_chatgpt::inspect(&mut page).await?;
    assert_eq!(state["turns"].as_array().unwrap().len(), 2);
    assert_eq!(state["turns"][0]["id"], "question-1");
    assert!(research_chatgpt::observation::user_turn_confirmed(
        &state, 0, prompt
    ));
    let response = research_chatgpt::observation::response_after(&state, 0, prompt)?.unwrap();
    assert_eq!(response["id"], "reply-1");
    assert_eq!(response["complete"], true);
    assert!(
        response["markdown"]
            .as_str()
            .unwrap()
            .contains("[docs](https://example.org/docs)")
    );
    assert!(research_chatgpt::observation::completion_candidate(
        &state, response
    ));
    let repeated = r##"<div data-chatgpt-search-message-ids="question-2"><div data-content-search-unit-key="fallback-turn-1:0:user">Find the official source.</div></div>"##;
    page.eval(&format!(
        "document.querySelector('main').insertAdjacentHTML('beforeend', {})",
        json!(repeated)
    ))
    .await?;
    let capture = research_chatgpt::capture_rendered_chat(&mut page).await?;
    assert_eq!(capture["turns"].as_array().unwrap().len(), 3);
    assert_eq!(capture["turns"][2]["id"], "question-2");
    assert_eq!(capture["missing_message_ids"], false);
    assert_eq!(capture["capture_limit_reached"], false);
    page.eval("document.querySelector('button[aria-label=\"Copy\"]').remove()")
        .await?;
    assert_eq!(
        research_chatgpt::inspect(&mut page).await?["turns"][1]["complete"],
        false
    );
    chrome.close(&page.id).await?;
    Ok(())
}

#[tokio::test]
#[ignore = "Requires installed Chrome; synthetic collapsed user bubble, no account message"]
async fn collapsed_user_message_ignores_decorative_ellipsis() -> Result<()> {
    let dir = fixture_dir()?;
    let chrome = Chrome::ensure(&dir, &Config::load(&dir)?).await?;
    let mut page = chrome.open("about:blank").await?;
    let prompt = "first paragraph\n\nsecond paragraph ends with a real ellipsis\u{2026}";
    let html = format!(
        "<div data-content-search-unit-key='turn:user' data-chatgpt-search-message-ids='user-1'>\
           <div data-user-message-bubble='true'><div><div>\
             <div data-search-result-target style='max-height:100px;overflow:hidden'><div><div style='white-space:pre-wrap'>{prompt}</div></div></div>\
             <span aria-hidden='true'>\u{2026}</span>\
           </div><button aria-expanded='false' data-thread-find-skip='true'>Show more</button></div></div>\
           <button aria-label='Copy message'>Copy</button>\
         </div>"
    );
    page.eval(&format!("document.body.innerHTML={}", json!(html)))
        .await?;
    let state = research_chatgpt::inspect(&mut page).await?;
    assert_eq!(state["turns"][0]["text"], prompt);
    assert!(research_chatgpt::observation::user_turn_confirmed(
        &state, 0, prompt
    ));
    let markdown = state["turns"][0]["markdown"].as_str().unwrap();
    assert_eq!(markdown.matches('\u{2026}').count(), 1);
    assert!(!markdown.contains("Show more"));
    chrome.close(&page.id).await?;
    Ok(())
}

#[tokio::test]
#[ignore = "Requires installed Chrome; synthetic reversed transcript, no account message"]
async fn reads_to_end_of_column_reverse_transcript() -> Result<()> {
    let dir = fixture_dir()?;
    let chrome = Chrome::ensure(&dir, &Config::load(&dir)?).await?;
    let mut page = chrome.open("about:blank").await?;
    let html = r#"<div style="height:120px;overflow-y:auto;display:flex;flex-direction:column-reverse">
      <main style="height:800px;flex:none">
        <div data-message-author-role="user" data-message-id="first">First turn</div>
        <div style="height:650px"></div>
        <div data-message-author-role="assistant" data-message-id="last">Last turn</div>
      </main></div>"#;
    page.eval(&format!("document.body.innerHTML={}", json!(html)))
        .await?;
    let capture = research_chatgpt::capture_rendered_chat(&mut page).await?;
    assert_eq!(capture["capture_limit_reached"], false);
    assert_eq!(capture["turns"].as_array().unwrap().len(), 2);
    assert_eq!(capture["turns"][0]["id"], "first");
    assert_eq!(capture["turns"][1]["id"], "last");
    assert_eq!(
        page.eval("document.querySelector('[style*=column-reverse]').scrollTop === 0")
            .await?,
        true
    );
    chrome.close(&page.id).await?;
    Ok(())
}

#[tokio::test]
#[ignore = "Requires installed Chrome; synthetic UI only, no account message"]
async fn composer_plugin_and_subscription_slider_fallback() -> Result<()> {
    let dir = fixture_dir()?;
    let chrome = Chrome::ensure(&dir, &Config::load(&dir)?).await?;
    let mut page = chrome.open("about:blank").await?;
    let html = r##"<nav><button type="button" onclick="throw new Error('Sidebar More must never be clicked')">More</button></nav><form>
      <div contenteditable="true" id="prompt-textarea"><p></p></div>
      <button type="button" id="plus" aria-label="Add files and more">+</button>
      <button type="button" id="effort" aria-haspopup="menu">Light</button>
      </form>
      <div id="tools" hidden><div tabindex="0" class="__menu-item" id="search"><span>Web search</span><span>Find real-time news and info</span></div></div>
      <div id="picker" hidden><div data-testid="composer-intelligence-picker-content">
        <div role="menuitem" tabindex="0" aria-label="Power" aria-describedby="announcement" id="power">
          <span role="slider" aria-valuenow="0" aria-valuemax="2" id="slider"></span>
        </div><span id="announcement">Light, 1 of 3.</span>
      </div></div>"##;
    page.eval(&format!("document.body.innerHTML={}", json!(html)))
        .await?;
    page.eval(r##"(() => {
      const $ = id=>document.getElementById(id);
      $('plus').onclick=()=>{$('tools').hidden=false};
      $('search').onclick=()=>{
        $('prompt-textarea').innerHTML='<p><span contenteditable="false" data-system-hint-type="search">Web search</span> </p>';
        $('tools').hidden=true;
      };
      $('effort').onclick=()=>{$('picker').hidden=false};
      document.addEventListener('keydown',e=>{if(e.key==='Escape')$('picker').hidden=true});
      $('power').onkeydown=e=>{
        const labels=['Light','Standard','High'];
        let n=Number($('slider').getAttribute('aria-valuenow'));
        n=Math.max(0,Math.min(2,n+(e.key==='ArrowRight'?1:e.key==='ArrowLeft'?-1:0)));
        $('slider').setAttribute('aria-valuenow',n);
        $('announcement').textContent=labels[n]+', '+(n+1)+' of 3.';
        $('effort').textContent=labels[n];
      };
    })()"##).await?;
    // An unintended forced mode must not silently change ordinary chat semantics.
    let action = include_str!("../src/scripts/action.js");
    assert_eq!(
        page.eval(&format!("({action})({{op:'open_tools'}})"))
            .await?["ok"],
        true
    );
    assert_eq!(
        page.eval(&format!(
            "({action})({{op:'select_mode',argument:'Search'}})"
        ))
        .await?["ok"],
        true
    );
    assert!(
        research_chatgpt::prepare(&mut page, &Config::default(), false, true)
            .await
            .unwrap_err()
            .to_string()
            .contains("research_mode_mismatch")
    );
    page.eval("document.querySelector('#prompt-textarea').innerHTML='<p><br class=\"ProseMirror-trailingBreak\"></p>'").await?;
    let prepared = research_chatgpt::prepare(&mut page, &Config::default(), false, true).await?;
    assert_eq!(prepared["reasoning"]["selected"], "High"); // Extra High is not offered by this fixture account.
    assert_eq!(prepared["state"]["mode"], serde_json::Value::Null);
    assert_eq!(prepared["state"]["composer_text"], "");
    research_chatgpt::fill(&mut page, "check source pls").await?;
    let state = research_chatgpt::inspect(&mut page).await?;
    assert_eq!(state["composer_text"], "check source pls");
    assert_eq!(state["mode"], serde_json::Value::Null);
    assert_eq!(chrome.window_state(&page.id).await?, "minimized");
    // A generic redirect is not deletion evidence; ChatGPT's explicit deleted-conversation notice is.
    let action = include_str!("../src/scripts/action.js");
    let check = format!("({action})({{op:'deleted',argument:'https://chatgpt.com/c/fixture'}})");
    page.eval("document.body.innerHTML='<p>Start a new chat.</p>'")
        .await?;
    assert_eq!(page.eval(&check).await?["ok"], false);
    page.eval("document.body.innerHTML='<p>Conversation has been deleted. Start a new chat.</p>'")
        .await?;
    assert_eq!(page.eval(&check).await?["ok"], true);
    page.eval("document.body.insertAdjacentHTML('beforeend','<a href=\"https://chatgpt.com/c/fixture\">Still listed</a>')").await?;
    assert_eq!(page.eval(&check).await?["ok"], false);
    chrome.close(&page.id).await?;
    Ok(())
}
