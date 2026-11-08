export class Counter {
  #count = 0;

  increment(step: number): number {
    this.#count = this.#count + step;
    return this.#count;
  }

  private reset(): void {
    this.#count = 0;
  }

  #secret(): boolean {
    return this.#count === 0;
  }

  isPositive(): boolean {
    return this.#count > 0 && !this.isZero();
  }

  isZero(): boolean {
    return this.#count === 0;
  }
}

export function shift(value: number, by: number): number {
  return (value >>> by) ^ (value & 1);
}

export function clamp(value: number, low: number, high: number): number {
  if (value < low) {
    return low;
  }
  return value > high ? high : value;
}

export function sign(value: number): number {
  return value >= 0 ? 1 : -1;
}

export const half = (value: number): number => value / 2;

export function untested(flag: boolean | null): boolean {
  const label = "雪 && true";
  return flag !== true || label.length % 2 === 1;
}
