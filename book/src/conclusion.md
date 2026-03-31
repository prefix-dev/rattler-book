# Conclusion

<span class="newthought">We started with an empty Cargo project</span> and ended up with a package manager that can discover packages, solve dependencies, install them into isolated environments, activate a shell, run commands, and build distributable packages. All in about 1500 lines of Rust.

Most of those lines are glue. The rattler crates do the heavy lifting: fetching repodata, running the SAT solver, linking packages into prefixes, generating activation scripts. Moonshot's job was to wire these pieces together behind a friendly CLI. That is the point of this book. You do not need to write a solver or an installer from scratch. The building blocks exist.

If you are thinking about building a package manager for your own language, the [Adapting to Your Language](deep-dive-adapting.md) deep dive maps out which pieces transfer directly and where you would need to diverge. The short version: chapters 1 through 9 are language-agnostic, and chapter 10 is where you make it your own.

I hope this book gave you a sense of how package managers work under the hood, and that the conda ecosystem is more approachable than it might have seemed. If you build something with rattler, I would love to hear about it.

Happy packaging.
