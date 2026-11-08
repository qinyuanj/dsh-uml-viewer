export function total(xs: number[]): number[] {
  const ys = filter(xs, odd);
  const zs = map(ys, inc);
  return list(zs);
}
