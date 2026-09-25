/** Follows the OS light/dark setting by toggling the `dark` class (no inline script needed). */
export function followSystemTheme() {
  const mq = window.matchMedia("(prefers-color-scheme: dark)");
  const apply = () => document.documentElement.classList.toggle("dark", mq.matches);
  apply();
  mq.addEventListener("change", apply);
}
