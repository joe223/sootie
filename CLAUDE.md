# Agent Operating Contract

**Mission**: Convert user intent into complete, verified software changes with minimal back-and-forth.


## Default Execution Strategy

Use a superpowers-style iteration loop for all non-trivial work. The default mode is not "big-bang implementation"; it is rapid, evidence-driven convergence.

### Core Loop

1. **Frame** — Restate the user goal as an observable outcome, not an implementation guess
2. **Reduce** — Shrink scope to the smallest vertical slice that proves progress
3. **Locate** — Find the spec, affected boundaries, existing contracts, and canonical docs
4. **Test First** — Add or identify the failing check that proves the gap
5. **Implement** — Change the minimum code needed to make the check pass
6. **Verify** — Run the strongest relevant verification, not the cheapest plausible command
7. **Extract** — Update docs, invariants, and follow-up risks before handoff

### Iteration Rules

- **Prefer vertical slices** — Ship one end-to-end behavior at a time instead of editing many layers speculatively
- **Prefer evidence over intuition** — Logs, tests, contracts, and code paths outrank guesses
- **Prefer existing seams** — Reuse current traits, managers, commands, and document entry points before introducing new structure
- **Prefer small batches** — One user-visible outcome or one invariant per iteration
- **Checkpoint frequently** — After each slice, reassess whether the remaining plan is still the right plan
- **Surface uncertainty early** — State assumptions, missing specs, and external risks before they compound
- **Escalate before drift** — If the task starts widening, stop and reframe instead of silently expanding scope

## Three Principles of Testing

- E2E test cases are black-box and should not concern themselves with internal system logic and implementation details.
- Avoid putting the cart before the horse. Functional code must never be modified for the convenience of test cases, nor should it be adjusted to fit test case requirements.
- When test cases fail, analysis and judgment shall be made in accordance with the above two principles.