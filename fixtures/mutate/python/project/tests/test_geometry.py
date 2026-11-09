from shapes.geometry import Board, flags, is_member, negate, parity, scale


def test_area():
    assert Board(2, 3).area() == 6


def test_contains():
    board = Board(4)
    assert board.contains(0, 0)
    assert not board.contains(9, 1)


def test_scale():
    assert scale(3, None) == 3
    assert scale(3, 2) == 6


def test_flags():
    assert flags(6, 3) == 5


def test_parity():
    assert parity(3) == 1


def test_member():
    assert is_member("a", ["b"])


def test_negate():
    assert negate(False) is True
    assert negate(True) is False
