({op, argument}) => {
  const visible = el => !!el && el.getClientRects().length > 0 && !el.closest('[inert],[hidden]');
  const label = el => (el.getAttribute('aria-label') || el.innerText || '').trim();
  const controls = () => [...document.querySelectorAll('button,[role="menuitem"],[role="option"],[role="menuitemradio"],.__menu-item[tabindex]')].filter(visible);
  const normalize = text => text.toLowerCase().replace(/[^a-z0-9]/g,'');
  // Project names are decorative URL suffixes; immutable project/chat IDs bind cleanup.
  const chatKey = value => {
    try {
      const url = new URL(value);
      if (url.origin !== 'https://chatgpt.com' || url.username || url.password) return null;
      const path = url.pathname.replace(/^(\/g\/g-p-[a-f0-9]{32})(?:-[^/]+)?(?=\/c\/)/i, '$1');
      return /^\/(?:g\/g-p-[^/]+\/)?c\/[^/]+$/.test(path) ? path : null;
    } catch { return null; }
  };
  const sameChat = (a, b) => chatKey(a) !== null && chatKey(a) === chatKey(b);
  const projectLandingFor = (value, expected) => {
    const chat = chatKey(expected);
    if (!chat?.startsWith('/g/')) return false;
    try {
      const url = new URL(value);
      if (url.origin !== 'https://chatgpt.com' || url.username || url.password) return false;
      const path = url.pathname.replace(/^(\/g\/g-p-[a-f0-9]{32})(?:-[^/]+)?(?=\/project$)/i, '$1');
      return path === chat.split('/').slice(0,3).join('/') + '/project';
    } catch { return false; }
  };
  const cleanupContext = expected => sameChat(location.href, expected) || projectLandingFor(location.href, expected);
  const rowChat = (value, expected) => sameChat(value, expected) || chatKey(value) === '/c/' + chatKey(expected)?.split('/').at(-1);
  const cleanupBinding = expected => {
    const binding = window.__researchCleanup;
    return cleanupContext(expected) && binding?.key === chatKey(expected) ? binding : null;
  };
  const available = () => controls().filter(el => !el.closest('nav,aside')).map(label).filter(Boolean);
  const click = el => {
    if (!el || el.disabled || el.getAttribute('aria-disabled') === 'true') return {ok:false, available:available()};
    const selected = label(el);
    for (const type of ['pointerdown','pointerup']) el.dispatchEvent(new PointerEvent(type, {bubbles:true,pointerId:1,pointerType:'mouse',button:0,buttons:type==='pointerdown'?1:0}));
    el.click(); return {ok:true,selected};
  };
  const find = pattern => controls().find(el => pattern.test(label(el)));
  const openCleanupMenu = (trigger, expected) => {
    if (!trigger || !visible(trigger)) return {ok:false};
    const previous = cleanupBinding(expected);
    if (previous?.trigger !== trigger) window.__researchCleanup = {key:chatKey(expected),trigger};
    return trigger.getAttribute('aria-expanded') === 'true' ? {ok:true} : click(trigger);
  };
  const reasoningTrigger = () => controls().find(el => el.closest('form') && el.hasAttribute('aria-haspopup') && /^(thinking|thinking time|thinking effort|reasoning|extended|standard|high|extra high|instant|light|maximum)$/i.test(label(el)));
  const composerSelector = '#prompt-textarea, [data-testid="composer-text-input"], form [role="textbox"][contenteditable="true"]';
  const modelTrigger = () => document.querySelector('[data-testid="model-switcher-dropdown-button"]') ||
    controls().find(el => el.closest('form')?.querySelector(composerSelector) && el.hasAttribute('aria-haspopup') && /^select chatgpt model$/i.test(label(el)));
  const picker = () => document.querySelector('[data-testid="composer-intelligence-picker-content"]') ||
    [...document.querySelectorAll('[role="menu"]')].find(el => visible(el) && el.querySelector('[role="menuitem"][aria-label="Power"] [role="slider"]'));
  const modelPicker = () => {
    if (visible(picker())) return picker();
    const trigger = modelTrigger();
    const controlled = document.getElementById(trigger?.getAttribute('aria-controls') || '');
    if (visible(controlled)) return controlled;
    return trigger?.getAttribute('aria-expanded') === 'true' ? [...document.querySelectorAll('[role="menu"],[role="listbox"]')].find(el => visible(el) && el.getAttribute('aria-labelledby') === trigger.id && trigger.id) : null;
  };
  if (op === 'open_model') {
    const trigger = modelTrigger() || reasoningTrigger();
    if (trigger?.getAttribute('aria-expanded') === 'true' || visible(picker())) return {ok:true};
    return click(trigger);
  }
  if (op === 'expand_model') {
    const trigger = find(/^select model$/i);
    return trigger?.getAttribute('aria-expanded') === 'true' ? {ok:true} : click(trigger);
  }
  if (op === 'model_status') {
    const selected = modelPicker()?.querySelector('[role="menuitemradio"][aria-checked="true"],[role="option"][aria-selected="true"]');
    return {selected:selected?.innerText.trim() || modelTrigger()?.innerText.trim() || null};
  }
  if (op === 'select_model') {
    const root = modelPicker();
    return click(controls().find(el => root?.contains(el) && !el.hasAttribute('aria-haspopup') && normalize(label(el).split('\n')[0]) === normalize(argument)));
  }
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
    const trigger = reasoningTrigger() || modelTrigger();
    // React can mount the picker after the trigger expands. Don't toggle it shut while waiting.
    if (trigger?.getAttribute('aria-expanded') === 'true') return {ok:true};
    return click(trigger);
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
    if (wanted === 'deepresearch' && composer?.querySelector('[data-id*="deep_research"],[data-system-hint-type="deep_research"]')) return {ok:true,selected:'Deep research'};
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
  if (op === 'focus' || op === 'clear_draft') {
    const composer = document.querySelector(composerSelector);
    if (!visible(composer)) return {ok:false};
    if (op === 'clear_draft' && composer.querySelector('[contenteditable="false"],[data-inline-selection-pill],button,[role="button"]')) return {ok:false,error:'draft_contains_controls'};
    composer.focus();
    // ProseMirror keeps an empty <p> inside the editable textbox. Put the
    // caret inside that block: a range after it does not accept insertText.
    const block = op === 'clear_draft' ? composer : composer.lastElementChild?.matches('p,div') ? composer.lastElementChild : composer;
    const range = document.createRange(); range.selectNodeContents(block);
    if (op === 'focus') range.collapse(false);
    const selection = window.getSelection(); selection.removeAllRanges(); selection.addRange(range);
    if (op === 'clear_draft') return {ok:document.execCommand('delete')};
    return {ok:true};
  }
  if (op === 'send') return click(document.querySelector('[data-testid="send-button"], #composer-submit-button') ||
    controls().find(el => el.closest('form')?.querySelector(composerSelector) && /^send$/i.test(label(el))));
  if (op === 'stop') return click(controls().find(el => !el.closest('nav,aside') &&
    (/stop-button|composer-stop-button/.test(el.dataset.testid || el.id) || /^stop(?: generating| streaming| response| research)?$/i.test(label(el)))));
  if (op === 'start_report') return click(controls().find(el => !el.closest('nav,aside') && /^start research$/i.test(label(el))));
  if (op === 'open_chat_menu') {
    // Bind either the open conversation or its exact row in the owned project.
    if (!cleanupContext(argument)) return {ok:false,error:'deletion_target_changed'};
    const expectedId = chatKey(argument).split('/').at(-1);
    const selector = projectLandingFor(location.href, argument) ? 'main a[href]' : 'a[href]';
    const link = [...document.querySelectorAll(selector)].find(a => rowChat(a.href, argument) &&
      !a.closest('form,[data-message-author-role],[data-content-search-unit-key]'));
    // Bind the actions button to the smallest row containing this one immutable
    // chat ID. Project cards/sidebar rows need not have any test IDs at all.
    for (let row = link?.parentElement, depth = 0; row && depth < 5; row = row.parentElement, depth++) {
      if (row.matches('nav,aside,main,header,[role="banner"]')) break;
      const ids = new Set([...row.querySelectorAll('a[href]')].map(a => chatKey(a.href)?.split('/').at(-1)).filter(Boolean));
      if (ids.size !== 1 || !ids.has(expectedId)) break;
      const buttons = [...row.querySelectorAll('button')].filter(el => visible(el) && el.getAttribute('aria-haspopup') === 'menu' &&
        (/^(chat actions|actions for .+|conversation options|chat options)$/i.test(label(el)) || /(?:conversation|chat).*options/i.test(el.dataset.testid || '')));
      if (buttons.length === 1) return openCleanupMenu(buttons[0], argument);
      if (buttons.length > 1) return {ok:false,error:'ambiguous_conversation_menu'};
    }
    if (!sameChat(location.href, argument)) return {ok:false,error:'owned_project_chat_row_unavailable'};
    const headers = [...document.querySelectorAll('[data-testid="conversation-options-button"]')].filter(visible);
    if (headers.length === 1) return openCleanupMenu(headers[0], argument);
    if (headers.length > 1) return {ok:false,error:'ambiguous_conversation_menu'};
    // Current accessible toolbar no longer carries a test ID. Require a unique
    // conversation toolbar, never a sidebar/project/message's generic More menu.
    const current = controls().filter(el => !el.closest('nav,aside,form,[data-message-author-role],[data-content-search-unit-key]') &&
      el.getAttribute('aria-haspopup') === 'menu' && /^(conversation options|chat options)$/i.test(label(el)) &&
      (el.closest('header,[role="banner"],[role="toolbar"]') ||
        [...(el.parentElement?.querySelectorAll('button') || [])].some(other => other !== el && /^share$/i.test(label(other)))));
    if (current.length === 1) return openCleanupMenu(current[0], argument);
    if (current.length > 1) return {ok:false,error:'ambiguous_conversation_menu'};
    return {ok:false,error:'owned_conversation_menu_unavailable'};
  }
  if (op === 'deletion_state') {
    const matching = cleanupContext(argument);
    const notices = [...document.querySelectorAll('[role="alert"],[role="status"],[data-sonner-toast],main *')].filter(el => visible(el) &&
      !el.closest('[data-message-author-role],[data-content-search-unit-key],article') &&
      (el.matches('[role="alert"],[role="status"],[data-sonner-toast]') || el.childElementCount === 0)).map(el => el.innerText);
    return {matching, unavailable:notices.some(text => /you (?:don’t|don't) have access to this conversation|unable to load conversation|conversation not found/i.test(text))};
  }
  if (op === 'delete_menu_item') {
    const binding = cleanupBinding(argument);
    const trigger = binding?.trigger;
    if (!trigger?.isConnected || trigger.getAttribute('aria-expanded') !== 'true') return {ok:false,error:'deletion_menu_binding_lost'};
    const menus = [...document.querySelectorAll('[role="menu"]')].filter(el => visible(el) &&
      (trigger.getAttribute('aria-controls') === el.id && el.id || trigger.id && el.getAttribute('aria-labelledby')?.split(' ').includes(trigger.id)));
    if (menus.length !== 1) return {ok:false,error:'owned_delete_menu_unavailable'};
    const candidates = menus.flatMap(menu => [...menu.querySelectorAll('[role="menuitem"],button')].filter(el => visible(el) && /^delete$/i.test(label(el))));
    if (candidates.length !== 1) return {ok:false,error:'ambiguous_or_missing_delete_menu'};
    const result = click(candidates[0]);
    if (result.ok) binding.confirming = true;
    return result;
  }
  if (op === 'confirm_delete') {
    const binding = cleanupBinding(argument);
    if (!binding?.confirming) return {ok:false,error:'deletion_dialog_binding_lost'};
    const dialogs = [...document.querySelectorAll('[role="dialog"], [role="alertdialog"]')].filter(el => visible(el) && /delete/i.test(el.innerText));
    if (dialogs.length !== 1) return {ok:false};
    const buttons = [...dialogs[0].querySelectorAll('button')].filter(el => visible(el) && /^delete(?: chat| conversation)?$/i.test(label(el)));
    if (buttons.length !== 1) return {ok:false};
    const result = click(buttons[0]);
    if (result.ok) { binding.confirmed = true; binding.confirming = false; }
    return result;
  }
  if (op === 'deleted') {
    const stillListed = [...document.querySelectorAll('a[href]')].some(a => rowChat(a.href, argument));
    const confirmation = window.__researchCleanup?.key === chatKey(argument) && window.__researchCleanup.confirmed &&
      [...document.querySelectorAll('[role="status"], [data-sonner-toast], [role="alert"]')].some(el => visible(el) && /(?:chat|conversation)(?: has been)? deleted/i.test(el.innerText)) ||
      [...document.querySelectorAll('body *')].some(el => visible(el) && el.childElementCount === 0 && /^Conversation has been deleted\. Start a new chat\.$/i.test(el.textContent.trim()));
    return {ok:confirmation && !stillListed && !sameChat(location.href, argument)};
  }
  return {ok:false,error:'unknown_operation'};
}
