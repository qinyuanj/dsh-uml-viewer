export function total(xs: number[]): number[] {
  const ys = filter(xs, odd);
  const zs = map(ys, inc);
  return list(zs);
}

export const amount = (items: number[]): number[] => {
  const kept = filter(items, even);
  const out = map(kept, dec);
  return list(out);
};

export class Box {
  run(xs: number[]): number[] {
    return xs;
  }
}

export function outer(xs: number[]): number[] {
  function inner(ys: number[]): number[] {
    return filter(ys, odd);
  }
  return inner(xs);
}
