// Sender avatars for the message list.
//
// The tint is keyed on the sender's *domain*, not the full address, so
// everyone writing from one organisation shares a color. That is what makes
// a list scannable without reading it: the eye learns "orange circle = work"
// long before it learns the individual names.

/** The domain of a From header, lowercased; null when it carries no address. */
export function senderDomain(from: string): string | null {
  // A display name may itself contain an @ ("alice@work" <alice@example.com>),
  // so the angle-addr wins whenever there is one.
  const angled = from.match(/<([^<>]*)>\s*$/);
  const address = (angled?.[1] ?? from).trim();
  const at = address.lastIndexOf("@");
  if (at < 0) {
    return null;
  }
  return address.slice(at + 1).trim().toLowerCase() || null;
}

/** FNV-1a — a stable, well-spread string hash. Math.imul keeps the multiply
 *  in 32-bit territory instead of drifting into float precision. */
function hash(value: string): number {
  let h = 0x811c9dc5;
  for (let i = 0; i < value.length; i += 1) {
    h ^= value.charCodeAt(i);
    h = Math.imul(h, 0x01000193);
  }
  return h >>> 0;
}

/** A stable fill for a domain's monogram.
 *
 *  why oklch: its lightness is perceptually uniform, so one fixed lightness
 *  holds the same contrast against the white monogram at every hue. In hsl
 *  the same lightness swings from readable (blue) to unreadable (yellow,
 *  green), which would have forced a hand-tuned palette instead. */
export function avatarColor(domain: string): string {
  return `oklch(48% 0.13 ${hash(domain) % 360}deg)`;
}
