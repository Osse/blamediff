# Topological sorting for [gitoxide](https://github.com/Byron/gitoxide)

This crate is a work in progress. It's an implementation of the
generation-based topological walk found in Git. See the links below.

Currently it's a standalone crate. My hope is to be able to submit a PR to
`gitoxide` eventually. Therefore it duplicates some of the stuff in `gitoxide`
in an attempt to make it easier to "slide in", for example the `Either` enum.

## Links:

* [Initial implementation in Git](https://github.com/git/git/commit/b45424181e)
* [Mailing list discussion](https://public-inbox.org/git/pull.25.git.gitgitgadget@gmail.com/)
* [Developer blog](https://devblogs.microsoft.com/devops/supercharging-the-git-commit-graph-iii-generations/)
