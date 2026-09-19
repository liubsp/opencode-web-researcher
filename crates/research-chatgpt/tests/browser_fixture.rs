use anyhow::Result;
use research_browser::Chrome;
use research_core::Config;
use serde_json::json;

/// Uses actual Chromium DOM semantics without contacting ChatGPT or sending account messages.
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
