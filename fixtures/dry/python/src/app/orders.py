def total(xs):
    ys = filter(xs, odd)
    zs = map(ys, inc)
    return list(zs)


class Order:
    def amount(self, items):
        kept = filter(items, even)
        out = map(kept, dec)
        return list(out)


def outer(xs):
    def inner(ys):
        return filter(ys, odd)

    return inner(xs)
