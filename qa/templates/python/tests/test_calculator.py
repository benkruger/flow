from src.calculator import Calculator


def test_add():
    calc = Calculator()
    assert calc.add(2, 3) == 5


def test_add_negative():
    calc = Calculator()
    assert calc.add(2, -3) == -1
