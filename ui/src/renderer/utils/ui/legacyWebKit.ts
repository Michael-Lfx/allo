type ArrayPredicate<T> = (value: T, index: number, array: T[]) => unknown;

function defineArrayMethod(
  name: string,
  implementation: (this: any[], ...args: any[]) => unknown
): void {
  if (name in Array.prototype) return;
  Object.defineProperty(Array.prototype, name, {
    configurable: true,
    writable: true,
    value: implementation,
  });
}

export function installLegacyWebKitPolyfills(): void {
  defineArrayMethod('toReversed', function (this: unknown[]) {
    return Array.from(this).reverse();
  });

  defineArrayMethod('toSorted', function (
    this: unknown[],
    compareFn?: (left: unknown, right: unknown) => number
  ) {
    return Array.from(this).sort(compareFn);
  });

  defineArrayMethod('findLast', function (
    this: any[],
    predicate: ArrayPredicate<any>,
    thisArg?: unknown
  ) {
    for (let index = this.length - 1; index >= 0; index -= 1) {
      const value = this[index];
      if (predicate.call(thisArg, value, index, this)) return value;
    }
    return undefined;
  });

  defineArrayMethod('findLastIndex', function (
    this: any[],
    predicate: ArrayPredicate<any>,
    thisArg?: unknown
  ) {
    for (let index = this.length - 1; index >= 0; index -= 1) {
      if (predicate.call(thisArg, this[index], index, this)) return index;
    }
    return -1;
  });
}

installLegacyWebKitPolyfills();
