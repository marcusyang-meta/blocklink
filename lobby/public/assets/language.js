// The URL is the language preference; no cookies or storage are needed.
for (const link of document.querySelectorAll('[data-language]')) {
  const destination = new URL(link.href);
  destination.hash = location.hash;
  link.href = destination.href;
}
addEventListener('hashchange', () => {
  for (const link of document.querySelectorAll('[data-language]')) {
    const destination = new URL(link.href);
    destination.hash = location.hash;
    link.href = destination.href;
  }
});
