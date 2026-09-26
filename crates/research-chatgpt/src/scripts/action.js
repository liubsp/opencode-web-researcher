({op, argument}) => {
  const visible = el => !!el && el.getClientRects().length > 0 && !el.closest('[inert],[hidden]');
  const label = el => (el.getAttribute('aria-label') || el.innerText || '').trim();
  const controls = () => [...document.querySelectorAll('button,[role="menuitem"],[role="option"],[role="menuitemradio"],.__menu-item[tabindex]')].filter(visible);
  const normalize = text => text.toLowerCase().replace(/[^a-z0-9]/g,'');
  const available = () => controls().filter(el => !el.closest('nav,aside')).map(label).filter(Boolean);
  const click = el => {
    if (!el || el.disabled || el.getAttribute('aria-disabled') === 'true') return {ok:false, available:available()};
    const selected = label(el);
    for (const type of ['pointerdown','pointerup']) el.dispatchEvent(new PointerEvent(type, {bubbles:true,pointerId:1,pointerType:'mouse',button:0,buttons:type==='pointerdown'?1:0}));
    el.click(); return {ok:true,selected};
  };
  const find = pattern => controls().find(el => pattern.test(label(el)));
  const reasoningTrigger = () => controls().find(el => el.closest('form') && el.hasAttribute('aria-haspopup') && /^(thinking|thinking time|thinking effort|reasoning|extended|standard|high|extra high|instant|light|maximum)$/i.test(label(el)));
  const composerSelector = '#prompt-textarea, [data-testid="composer-text-input"], form [role="textbox"][contenteditable="true"]';
  const modelTrigger = () => document.querySelector('[data-testid="model-switcher-dropdown-button"]') ||
    controls().find(el => el.closest('form')?.querySelector(composerSelector) && el.hasAttribute('aria-haspopup') && /^select chatgpt model$/i.test(label(el)));
  const picker = () => document.querySelector('[data-testid="composer-intelligence-picker-content"]') ||
    [...document.querySelectorAll('[role="menu"]')].find(el => visible(el) && el.querySelector('[role="menuitem"][aria-label="Power"] [role="slider"]'));
  if (op === 'open_model') return click(modelTrigger() || reasoningTrigger());
  if (op === 'expand_model') return click(find(/^select model$/i));
  if (op === 'open_tools') {
    const plus = document.querySelector('[data-testid="composer-plus-btn"]');
    const trigger = visible(plus) ? plus : controls().find(el =>
      el.closest('form')?.querySelector(composerSelector) &&
      /^(tools|add files and more|add photos and files|more)$/i.test(label(el)));
    if (trigger?.getAttribute('aria-expanded') === 'true') return {ok:true};
    return click(trigger);
  }
  if (op === 'open_reasoning') {
    if (visible(picker())) return {ok:true};
    return click(reasoningTrigger() || modelTrigger());
  }
  if (op === 'reasoning_status' || op === 'reasoning_focus') {
    const power = picker()?.querySelector('[aria-label="Power"]');
    const slider = power?.querySelector('[role="slider"]');
    if (!slider) return {ok:false};
    if (op === 'reasoning_focus') power.focus();
    const announcement = (power.getAttribute('aria-describedby') || '').split(' ').map(id=>document.getElementById(id)?.textContent || '').find(text=>/\d+ of \d+/.test(text)) || '';
    return {ok:true,selected:announcement.replace(/,\s*\d+ of \d+\.?$/,''),index:Number(slider.getAttribute('aria-valuenow')),max:Number(slider.getAttribute('aria-valuemax')),
      model:picker()?.querySelector('[role="menuitemradio"][aria-checked="true"]')?.innerText.trim() || null};
  }
  if (op === 'select') {
    for (const wanted of argument) {
      const item = controls().find(el => !el.hasAttribute('aria-haspopup') && normalize(label(el).split('\n')[0]) === normalize(wanted));
      if (item) return click(item);
    }
    return {ok:false,available:available()};
  }
  if (op === 'select_mode') {
    const wanted = normalize(argument);
    const composer = document.querySelector(composerSelector);
    if (wanted === 'search' && composer?.querySelector('[data-system-hint-type="search"]')) return {ok:true,selected:'Web search'};
    const names = wanted === 'search' ? ['web search','search the web'] : ['deep research'];
    const leaf = [...document.querySelectorAll('span')].find(el => visible(el) && el.childElementCount === 0
      && names.includes(el.textContent.trim().toLowerCase()) && !el.closest('nav,aside'));
    const pluginItem = leaf?.closest('.__menu-item[tabindex]');
    if (pluginItem) return click(pluginItem);
    // Never click the sidebar's chat-history Search button instead of composer web Search.
    const item = controls().find(el => {
      const inComposer = el.closest('form')?.querySelector(composerSelector);
      const inMenu = el.closest('[role="menu"], [role="listbox"]');
      return (inComposer || inMenu) && (normalize(label(el).split('\n')[0]) === wanted ||
        (wanted === 'search' && ['searchtheweb','websearch'].includes(normalize(label(el).split('\n')[0]))));
    });
    if (item?.getAttribute('aria-pressed') === 'true') return {ok:true,selected:label(item)};
    return click(item);
  }
  if (op === 'focus') {
    const composer = document.querySelector(composerSelector);
    if (!visible(composer)) return {ok:false};
    composer.focus();
    // ProseMirror keeps an empty <p> inside the editable textbox. Put the
    // caret inside that block: a range after it does not accept insertText.
    const block = composer.lastElementChild?.matches('p,div') ? composer.lastElementChild : composer;
    const range = document.createRange(); range.selectNodeContents(block); range.collapse(false);
    const selection = window.getSelection(); selection.removeAllRanges(); selection.addRange(range);
    return {ok:true};
  }
  if (op === 'send') return click(document.querySelector('[data-testid="send-button"], #composer-submit-button') ||
    controls().find(el => el.closest('form')?.querySelector(composerSelector) && /^send$/i.test(label(el))));
  if (op === 'start_report') return click(find(/^start research$/i));
  if (op === 'open_chat_menu') {
    // Project chats may be absent from the sidebar's truncated history. The header
    // menu belongs to the open conversation, so require the exact saved URL first.
    if (location.href.split('?')[0] !== argument) return {ok:false,error:'deletion_target_changed'};
    const header = document.querySelector('[data-testid="conversation-options-button"]');
    if (visible(header)) return click(header);
    const link = [...document.querySelectorAll('a[href]')].find(a => a.href.split('?')[0] === argument);
    const row = link?.closest('[data-testid]') || link?.parentElement;
    const button = row?.querySelector('button[data-testid*="options"], button[aria-haspopup="menu"]');
    return click(button);
  }
  if (op === 'confirm_delete') {
    const dialog = document.querySelector('[role="dialog"], [role="alertdialog"]');
    if (!dialog || !/delete/i.test(dialog.innerText)) return {ok:false};
    return click([...dialog.querySelectorAll('button')].find(el => /^delete$/i.test(label(el))));
  }
  if (op === 'deleted') {
    const stillListed = [...document.querySelectorAll('a[href]')].some(a => a.href.split('?')[0] === argument);
    const confirmation = [...document.querySelectorAll('[role="status"], [data-sonner-toast], [role="alert"]')].some(el => /(?:chat|conversation)(?: has been)? deleted/i.test(el.innerText)) ||
      [...document.querySelectorAll('body *')].some(el => visible(el) && el.childElementCount === 0 && /^Conversation has been deleted\. Start a new chat\.$/i.test(el.textContent.trim()));
    return {ok:confirmation && !stillListed && location.href.split('?')[0] !== argument};
  }
  return {ok:false,error:'unknown_operation'};
}
