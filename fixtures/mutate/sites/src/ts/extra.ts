type Flag = true | 0 | 1;

export class Store<T> {
  #items: T[] = [];
  private hidden = 1;

  #grow(value: T): number {
    return this.#items.push(value) - 1;
  }

  private drop(index: number): boolean {
    return delete this.#items[index] || void 0 === undefined;
  }

  protected size(): number {
    return this.#items.length >>> 0;
  }

  static make<U>(value: U): Store<U> {
    const store = new Store<U>();
    store.#grow(value);
    return store;
  }

  get empty(): boolean {
    return !this.#items.length;
  }
}

export function checks(a: any, b: number): boolean {
  const flag: Flag = 1;
  return typeof a === "string" && a instanceof Object || "k" in a || -b < +b && ~b !== 0 && a!.x == null || a ?? b ** 2 > 1;
}

export const arrow = (a: number) => a != 1 ? a % 2 : -1;

const handlers = {
  run(a: number) {
    return a <= 1;
  },
  stop: function (a: number) {
    return a >= 0;
  },
};

export default function (a: number) {
  return a > 0;
}
