export function sum(xs: number[]): number[] {
  const ys = filter(xs, odd);
  const zs = map(ys, inc);
  return list(zs);
}

export const subtotal = (items: number[]): number[] => {
  const kept = filter(items, even);
  const out = map(kept, dec);
  return list(out);
};

export class Panel {
  walk(xs: number[]): number[] {
    return xs;
  }
}

export function outer(xs: number[]): number[] {
  function inner(ys: number[]): number[] {
    return filter(ys, odd);
  }
  return inner(xs);
}
