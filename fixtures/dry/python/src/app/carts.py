class Cart:
    def total(self, xs):
        ys = filter(xs, odd)
        zs = map(ys, inc)
        return list(zs)


def amount(items):
    kept = filter(items, even)
    out = map(kept, dec)
    return list(out)
