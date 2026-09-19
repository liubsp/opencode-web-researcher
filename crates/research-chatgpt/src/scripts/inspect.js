(() => {
  const visible = el => !!el && el.getClientRects().length > 0 && !el.closest('[inert],[hidden]');
  const composer = document.querySelector('#prompt-textarea, [data-testid="composer-text-input"]');
  const buttons = [...document.querySelectorAll('button')].filter(visible);
  const label = el => (el.getAttribute('aria-label') || el.innerText || '').trim();
  const turns = [...document.querySelectorAll('[data-message-author-role]')];
  const editableText = node => {
    if (node.nodeType === Node.TEXT_NODE) return node.textContent.replace(/[\u200b\ufeff]/g,'');
    if (node.nodeType !== Node.ELEMENT_NODE || node.getAttribute('contenteditable') === 'false' || node.hasAttribute('data-inline-selection-pill') || node.matches('button,[role="button"]')) return '';
    const text = [...node.childNodes].map(editableText).join('');
    return /^(P|DIV|BR)$/.test(node.tagName) ? text + '\n' : text;
  };
  function markdown(node) {
    if (node.nodeType === Node.TEXT_NODE) return node.textContent;
    if (node.nodeType !== Node.ELEMENT_NODE) return '';
    const tag = node.tagName.toLowerCase();
    if (['button', 'svg', 'script', 'style'].includes(tag)) return '';
    if (tag === 'pre') return '\n```\n' + (node.querySelector('code')?.textContent || node.textContent) + '\n```\n';
    const text = [...node.childNodes].map(markdown).join('');
    if (tag === 'a') {
      const href = node.href;
      return /^https?:/.test(href) ? `[${text || href}](${href})` : text;
    }
    if (/^h[1-6]$/.test(tag)) return '\n' + '#'.repeat(Number(tag[1])) + ' ' + text + '\n';
    if (tag === 'li') return '\n- ' + text.trim() + '\n';
    if (tag === 'table') {
      const rows = [...node.querySelectorAll('tr')].map(row => [...row.children].map(cell => markdown(cell).trim().replace(/\|/g,'\\|').replace(/\n/g,'<br>')));
      if (!rows.length) return '';
      const line = cells => '| ' + cells.join(' | ') + ' |';
      return '\n' + [line(rows[0]), line(rows[0].map(()=> '---')), ...rows.slice(1).map(line)].join('\n') + '\n';
    }
    if (tag === 'br') return '\n';
    if (tag === 'code') return '`' + text + '`';
    if (tag === 'strong') return '**' + text + '**';
    if (tag === 'tr') return '\n| ' + [...node.children].map(markdown).join(' | ') + ' |';
    return ['p','div','section','ul','ol','table','blockquote'].includes(tag) ? '\n' + text + '\n' : text;
  }
  return {
    url: location.href.split('?')[0], composer: visible(composer),
    composer_text: composer ? (composer.value ?? editableText(composer)).trim() : '',
    mode: composer?.querySelector('[data-system-hint-type="search"]') ? 'search' :
      composer?.querySelector('[data-id*="deep_research"],[data-system-hint-type="deep_research"]') ? 'deep_research' : null,
    login_required: buttons.some(b => /^(log in|sign in|sign up)$/i.test(label(b))) && !document.querySelector('[data-testid="profile-button"]'),
    busy: buttons.some(b => /stop (generating|streaming|response)|^stop$/i.test(label(b)) || /stop-button|composer-stop-button/.test(b.dataset.testid || b.id)),
    turns: turns.map(el => ({role:el.getAttribute('data-message-author-role'), text:el.getAttribute('data-message-author-role') === 'user' ? editableText(el).trim() : el.innerText,
      markdown:markdown(el).replace(/\n{3,}/g,'\n\n').trim(),
      complete: el.getAttribute('data-message-author-role') === 'assistant' && !el.querySelector('.streaming-animation') &&
        !!el.closest('.agent-turn,article,[data-testid^="conversation-turn-"]')?.querySelector('[data-testid="copy-turn-action-button"], button[aria-label="Copy response"]'),
      citations:[...el.querySelectorAll('a[href]')].filter(a => /^https?:/.test(a.href)).map(a => ({url:a.href,title:a.innerText}))})),
    controls: buttons.map(label).filter(Boolean)
  };
})()
