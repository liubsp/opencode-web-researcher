use anyhow::Result;
use research_browser::Chrome;
use research_core::Config;
use serde_json::json;

/// Uses actual Chromium DOM semantics without contacting ChatGPT or sending account messages.
#[tokio::test]
#[ignore = "Requires installed Chrome; synthetic deletion controls only"]
async fn deletion_menu_uses_current_chat_header_without_sidebar_history() -> Result<()> {
    let dir = research_core::data_dir()?;
    let chrome = Chrome::ensure(&dir, &Config::load(&dir)?).await?;
    let mut page = chrome.open("about:blank").await?;
    let html = r##"<nav><a href="https://chatgpt.com/c/other">Other chat</a>
      <button aria-haspopup="menu" onclick="window.wrong=true">More</button></nav>
      <header><button data-testid="conversation-options-button" aria-label="More"
      onclick="window.opened=(window.opened||0)+1">...</button></header>"##;
    page.eval(&format!("document.body.innerHTML={}", json!(html)))
        .await?;
    let action = include_str!("../src/scripts/action.js");
    let wrong =
        format!("({action})({{op:'open_chat_menu',argument:'https://chatgpt.com/c/other'}})");
    assert_eq!(page.eval(&wrong).await?["ok"], false);
    assert_eq!(
        page.eval("window.opened === undefined && window.wrong === undefined")
            .await?,
        true
    );
    let current = format!("({action})({{op:'open_chat_menu',argument:location.href}})");
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
#[ignore = "Requires installed Chrome; run explicitly for the browser compatibility check"]
async fn extraction_and_input_against_a_chrome_fixture() -> Result<()> {
    let dir = research_core::data_dir()?;
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
    let dir = research_core::data_dir()?;
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
#[ignore = "Requires installed Chrome; synthetic current turn markup, no account message"]
async fn current_turn_markup_confirms_submission_and_response() -> Result<()> {
    let dir = research_core::data_dir()?;
    let chrome = Chrome::ensure(&dir, &Config::load(&dir)?).await?;
    let mut page = chrome.open("about:blank").await?;
    let prompt = "Find the official source.";
    let html = r##"<main><div class="group"><div><div>
      <div data-content-search-unit-key="fallback-turn-0:0:user"><div data-user-message-bubble="true">Find the official source.</div>
      <button aria-label="Copy message">Copy</button></div>
      <div><div data-content-search-unit-key="fallback-turn-0:2:assistant" data-chatgpt-search-message-ids="reply-1">
      <h4>ChatGPT said:</h4><div data-markdown-text-style="assistant-message"><p>See <a href="https://example.org/docs">docs</a>.</p></div>
      </div></div></div><button aria-label="Copy">Copy</button></div></main>"##;
    page.eval(&format!("document.body.innerHTML={}", json!(html)))
        .await?;
    let state = research_chatgpt::inspect(&mut page).await?;
    assert_eq!(state["turns"].as_array().unwrap().len(), 2);
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
#[ignore = "Requires installed Chrome; synthetic UI only, no account message"]
async fn composer_plugin_and_subscription_slider_fallback() -> Result<()> {
    let dir = research_core::data_dir()?;
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
    // Exercise explicit Search menu selection separately; ordinary preparation leaves mode alone.
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
    let prepared = research_chatgpt::prepare(&mut page, &Config::default(), false, true).await?;
    assert_eq!(prepared["reasoning"]["selected"], "High"); // Extra High is not offered by this fixture account.
    assert_eq!(prepared["state"]["mode"], "search");
    assert_eq!(prepared["state"]["composer_text"], "");
    research_chatgpt::fill(&mut page, "check source pls").await?;
    let state = research_chatgpt::inspect(&mut page).await?;
    assert_eq!(state["composer_text"], "check source pls");
    assert_eq!(state["mode"], "search");
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
