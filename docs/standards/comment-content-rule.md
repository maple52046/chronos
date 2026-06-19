# Comment Content Rule

Comments must document information that code cannot reliably express. This rule
applies to every comment in the repository (source code, config, build
scripts).

A comment is allowed only if it belongs to one of these semantic categories:

- **Intent**: why this code exists
- **Rationale**: why this approach was chosen
- **Contract**: what callers or maintainers must guarantee
- **Invariant**: what must always remain true
- **Constraint**: what external or internal limitation shapes this code
- **Risk**: what can break if this code is changed incorrectly
- **Side Effect**: what state, system, or external dependency is affected
- **Domain Mapping**: how code concepts map to product/system/domain concepts
- **Operational Context**: how this affects deployment, debugging, tracing,
  recovery, or observability

Do not write comments that merely:

- repeat code
- translate names into prose
- narrate obvious control flow
- describe syntax
- restate simple assignments
- add decorative section markers
- preserve irrelevant history
- express vague intent without a concrete constraint, risk, or contract

Before adding a comment, answer:

1. Which semantic category does this comment belong to?
2. What future mistake does this comment help prevent?
3. Why is this information better expressed as a comment than as a name, type,
   test, or refactor?
4. Is this comment stable under normal implementation changes?

If you cannot answer these questions, do not add the comment.

> Note: API documentation comments (the doc-comment system of the project's
> language, e.g. docstrings / Javadoc / rustdoc / JSDoc) are a contract for
> callers and are always expected where the language style standard requires
> them. The bans above target *narration*, not legitimate API documentation.
