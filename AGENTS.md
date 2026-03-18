# Prose Style Rules

When writing or editing book chapters, avoid common LLM prose tells. The full reference is at https://git.eeqj.de/sneak/prompts/src/branch/main/prompts/LLM_PROSE_TELLS.md but the key rules are:

**Em-dashes**: Do not use em-dashes (—). Replace with the punctuation that belongs there: commas, semicolons, colons, parentheses, or periods. If you can't identify which one fits, the sentence needs restructuring.

**Overused intensifiers**: Avoid "crucial", "vital", "robust", "comprehensive", "fundamental", "arguably", "straightforward", "noteworthy", "realm", "landscape", "leverage" (as verb), "delve", "tapestry", "multifaceted", "nuanced", "pivotal", "unprecedented", "navigate", "foster", "underscores", "resonates", "embark", "streamline", "spearhead". Use plainer words or delete.

**Filler adverbs**: Remove "importantly", "essentially", "fundamentally", "ultimately", "inherently", "particularly", "increasingly", "dramatically" when the sentence works without them.

**Elevated register**: Use "use" not "utilize", "start" not "commence", "help" not "facilitate", "show" not "demonstrate", "try" not "endeavor", "change" not "transform", "make" not "craft".

**Two-clause compound sentences**: Vary sentence structure. Not every sentence should be "[clause], [conjunction] [clause]". Mix in single-clause sentences, sentences starting with subordinate clauses, mid-sentence relative clauses.

**Pivot paragraphs**: Delete one-sentence paragraphs that exist only to transition ("But here's where it gets interesting.", "There is a lot going on here."). Get to the point.

**Parenthetical qualifiers**: Remove "There is, however, ..." / "This is, of course, ..." / "There are, to be fair, ..." unless the qualifier actually changes the argument.

**Triple constructions**: Don't always list exactly three parallel items. Sometimes use two, sometimes four or more, sometimes break the grammatical parallelism.

**Colon elaborations**: If the clause before the colon adds nothing, start the sentence with the substance after the colon.

**"Not X but Y" pivots**: Rephrase without the negation-then-correction structure.

**Unnecessary trailing clauses**: If the last third of a sentence restates what the first two-thirds already said, end the sentence earlier.

**Connector addiction**: Don't start consecutive paragraphs with "However", "Furthermore", "Moreover", "Additionally", "That said". Start with the subject instead.

**"Production"**: Avoid using "production" (e.g. "production-ready", "production code"). It sounds out of place in a book about Lua packaging.
