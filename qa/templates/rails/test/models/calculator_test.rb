require "minitest/autorun"
require_relative "../../app/models/calculator"

class CalculatorTest < Minitest::Test
  def test_add
    calc = Calculator.new
    assert_equal 5, calc.add(2, 3)
  end

  def test_add_negative
    calc = Calculator.new
    assert_equal(-1, calc.add(2, -3))
  end
end
