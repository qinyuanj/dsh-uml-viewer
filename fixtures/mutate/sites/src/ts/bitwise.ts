export function mix(a: number, b: number) {
  return ((a ^ b) & (a | b)) << ((a >> b) % b);
}
