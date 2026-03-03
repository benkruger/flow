# Testing Gotchas

## Function Alias Tautology

When converting a subprocess test to in-process and the converted test
compares two function calls (`result == other_module.f(same_args)`),
check first whether both names refer to the same object (`f is g`).
If they are the same, the comparison is tautological — replace with
behavioral assertions (`isinstance`, content checks, specific values).
