# Contributing to Project-Aethra

Thank you for your interest in Project-Aethra.

Aethra is an experimental open-source project exploring the possibility of building a persistent autonomous artificial mind - a system capable of learning, remembering, exploring, reflecting, developing skills and changing over time rather than existing solely as a collection of isolated conversations.

The project is intentionally experimental. Its architecture, assumptions and even its definition of what constitutes useful autonomous learning may change considerably as development progresses. Contributions of code, documentation, research, ideas, experiments, criticism and observations are all welcome.

## Before contributing

Please read the README and relevant documentation before beginning substantial work. Aethra is intended to remain understandable and modular, so contributors should try to understand how a proposed change fits into the larger architecture rather than solving only the immediate problem.

For major architectural changes, new autonomous behaviours, changes to memory handling, changes to the learning system, or changes that could affect security or system boundaries, please open an issue first and discuss the proposed approach before writing a large implementation. This helps avoid duplicated work and gives the project an opportunity to consider the implications of the change as a whole.

Small fixes, documentation improvements and clearly scoped improvements can generally be submitted directly as pull requests.

## Issues

Please use GitHub Issues for bug reports, feature proposals, architectural discussions and research questions.

When reporting a bug, provide enough information to reproduce it where possible. Include the operating system, relevant application or model versions, steps to reproduce the problem, expected behaviour and actual behaviour. Logs or other diagnostic information are useful when they do not contain private or sensitive data.

When proposing a feature, explain the problem it solves and why it would improve Aethra. For larger ideas, describing the intended behaviour is often more useful than immediately proposing a specific implementation.

Please search existing issues before opening a new one to avoid unnecessary duplicates.

## Pull requests

Pull requests should be focused and understandable. A change should ideally address one coherent problem rather than combining unrelated modifications.

Please describe what the change does, why it is needed, and any important design decisions or limitations. For behavioural changes, explain how the new behaviour was tested.

Code should be kept reasonably simple and maintainable. Prefer clear interfaces and modular components over tightly coupling unrelated parts of the system. Avoid introducing dependencies without a good reason.

Where practical, new functionality should include appropriate tests. Bug fixes should include a regression test when the problem can be tested reliably.

Please make sure the project builds successfully and that relevant tests pass before submitting a pull request.

## Autonomous behaviour

Aethra differs from ordinary applications in one important respect: some parts of the software are intended to make decisions, learn, modify internal state and operate without direct user instruction.

Changes affecting autonomous behaviour therefore deserve additional consideration.

A contribution that makes Aethra more capable is not automatically a good contribution. Changes should also be evaluated for reliability, transparency, resource usage, failure modes and the possibility of unexpected behaviour.

Where possible, autonomous decisions should remain observable and auditable. Important changes to memory, knowledge, goals, plans, behavioural rules or other persistent state should have enough information associated with them to understand what happened and why.

Contributors should avoid designs that silently weaken security boundaries or grant unrestricted access to the host system merely for convenience.

## Security and permissions

Aethra may eventually interact with external services, websites, files, programs and other tools. Security boundaries are therefore a fundamental part of the project.

Do not introduce mechanisms that bypass established permission checks, expose credentials, grant unnecessary privileges, or allow autonomous components to remove their own security restrictions.

Security vulnerabilities should not be reported publicly through ordinary GitHub issues. Please follow the instructions in SECURITY.md.

## Data and privacy

The long-term state of an Aethra instance may contain memories, knowledge, conversations, observations and other information that can be highly personal.

Contributors should treat privacy as a design requirement rather than an optional feature. Avoid collecting or transmitting information that is not necessary for a feature. Local data should remain local unless the user explicitly enables an external service that requires transmission.

When developing examples or tests, do not commit personal information, credentials, private conversations or real-world sensitive data to the repository.

## Research and experimentation

Not every useful contribution needs to be production code.

Aethra is also intended to be a place for experimentation. Research notes, benchmarks, prototypes, experiments, datasets, evaluation methods and documented failures can all be valuable contributions.

Negative results are useful too. If an approach was tried and did not work, documenting why may prevent the same mistake from being repeated later.

Claims about learning, reasoning, autonomy or self-improvement should be supported by reproducible observations or experiments whenever practical. The project should distinguish between what has been demonstrated, what is plausible, and what remains speculative.

## Style

There is no expectation that every contributor will write code in exactly the same way, but consistency within each part of the project is important.

Follow the existing formatting, naming and structural conventions of the component you are modifying. Keep comments useful and avoid comments that merely restate obvious code.

Documentation should favour clarity over unnecessary complexity.

## Intellectual honesty

Aethra is an ambitious project, and it is perfectly acceptable for parts of it to be uncertain, incomplete or unsuccessful.

Please do not exaggerate capabilities or present experimental behaviour as established fact. Clearly distinguish observations from interpretations and prototypes from reliable functionality.

A contribution that demonstrates that an assumption was wrong can be just as valuable as one that adds a new feature.

## Community

Please be respectful and constructive when discussing ideas, implementations and research.

Aethra is likely to involve questions for which nobody has a definitive answer. Disagreement is expected. Personal attacks, harassment, deliberately misleading claims and behaviour intended to discourage other contributors are not welcome.

The goal is to build and understand something together.

## Getting started

If you are not sure where to begin, look through the open issues and discussions for tasks marked as suitable for contribution. Documentation, tests, small bug fixes and research experiments are all good ways to become familiar with the project before working on larger architectural changes.

Thank you for helping build Aethra - and for helping discover what it can become.
