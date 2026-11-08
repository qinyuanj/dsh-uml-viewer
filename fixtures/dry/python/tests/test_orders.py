def total(xs):
    ys = filter(xs, odd)
    zs = map(ys, inc)
    return list(zs)
