/**
 * Trailing-edge debounce: `fn` runs once, `ms` after the last call, with the
 * last call's arguments. Earlier pending calls are dropped.
 */
export function debounce<A extends unknown[]>(
  fn: (...args: A) => void,
  ms: number,
): (...args: A) => void {
  let timer: ReturnType<typeof setTimeout> | undefined;
  return (...args: A) => {
    clearTimeout(timer);
    timer = setTimeout(() => fn(...args), ms);
  };
}
