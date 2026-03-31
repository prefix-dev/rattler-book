# Conclusion

<span class="newthought">We started with an empty Cargo project</span> and ended up with a package manager that can discover packages, solve dependencies, install them into isolated environments, activate a shell, run commands, and build distributable packages. All in under 2000 lines of Rust.

Most of those lines are glue. The rattler crates do the heavy lifting: fetching repodata, running the SAT solver, linking packages into prefixes, generating activation scripts. Moonshot's job was to wire these pieces together behind a friendly CLI. That is the point of this book. You do not need to write a solver or an installer from scratch. The building blocks exist, and are yours to build upon.

If you are thinking about building a package manager for your own language, the [Adapting to Your Language](deep-dive-adapting.md) deep dive maps out which pieces transfer directly and where you would need to diverge. The short version: [chapters 1](ch01-what-is-a-package-manager.md) through [9](ch09-run.md) are mostly language-agnostic, and [chapter 10](ch10-build.md) is where you might need to make modifications the most.

I hope this book gave you a sense of how package managers work under the hood, and have hopefully convinced, you the reader, that the conda ecosystem is interesting to build upon. If you build something with rattler, I and the rest of prefix.dev would love to hear about it.

Happy packaging.
