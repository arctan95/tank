document.addEventListener('DOMContentLoaded', () => document.body.style.cursor = 'none');

document.addEventListener('contextmenu', event => event.preventDefault());

document.addEventListener('keydown', (event) => {
  window.ipc.postMessage(event.code);
});