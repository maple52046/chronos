Please consolidate the AI plan files under the current plan manuscripts directory.

Default source directory:
./manuscripts

Default output directory:
the parent directory of ./manuscripts

Your task is to read all AI plan documents in the source directory, extract valuable long-term planning content, and generate a new consolidated markdown plan.

Focus on preserving:
1. planning content
2. architectural decisions
3. constraints and rules
4. implementation boundaries
5. confirmed user preferences
6. open questions and pending decisions

Do not simply concatenate the files.
Do not modify, delete, or move the original draft files.
Do not introduce large new designs that are not supported by the source plans.
If plans conflict, prefer the newer or more explicit decision. If the conflict cannot be resolved, place it under Open Questions.

The output filename must follow this format:

yyyyMMDD_HHMM_<title>.md

Use the current execution time for yyyyMMDD_HHMM.
Use lowercase snake_case for <title>.
If the topic is unclear, use:

yyyyMMDD_HHMM_consolidated_ai_plans.md

The output markdown should include these sections:

1. Purpose
2. Source Scope
3. Consolidated Background
4. Confirmed Decisions
5. Architecture and Design Principles
6. Functional Scope
7. Constraints and Rules
8. Data Model and Format Notes
9. CLI / API / Config Notes
10. Implementation Plan
11. Non-goals
12. Open Questions
13. Future Work

After writing the file, report:
- the generated file path
- the source directory
- the number of files processed
- the main themes consolidated