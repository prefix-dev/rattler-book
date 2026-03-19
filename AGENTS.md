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


# Literate programming

We are using entangled for literate programming, this means all the source code needs to be available in the book and the source code can always be generated from the book.
This is also useful, for working with LLM's to make sure to combat hallucinations.

Most of the tasks pertaining to this workflow are available in the `pixi.toml`.

## Entangled stitch roundtrip

The book is the source of truth. Source files are generated from it via
`pixi run tangle`. If you edit source files directly, run `pixi run stitch`
to push changes back into the book, then `pixi run tangle` again to verify the
roundtrip is stable.

Entangled uses `// ~/~ begin/end` markers in tangled source files to track which
code belongs to which named block. These markers are generated automatically by
tangle. If a file-level code block in the markdown uses `<<block-name>>`
references, tangle emits matching sub-block markers in the source file so that
stitch can reconstruct the references. Without these markers, stitch replaces
the `<<references>>` with literal expanded code, destroying the book structure.

CI runs a stitch roundtrip check after tangle. If it fails, the usual fix is to
ensure you edited the **book** (not just the source) and ran tangle.

Make sure to run:
