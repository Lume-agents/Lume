# GitHub in Lume

## Repository Inspector

- **Chat repository:** click the repository/branch below the chat title, or choose **Repository** in the Inspector. The same Inspector is available in Workspace and Orb.
- **Account activity:** in the repository Inspector, choose **Overview → My account**. The calendar includes contributions reported by GitHub. Expand its footer to see the three repositories with the most commit contributions. There is no separate GitHub destination.

An icon navigation exposes Overview, Changes, Commits, PRs, Issues and Actions directly. Repository tools include shared working-tree changes, bounded text previews with diff gutters, the last 20 commits, up to 15 open PRs and 15 open issues, HEAD checks and the last 10 Actions runs. Repository selection follows the current chat. In Orb, the Inspector scrolls within the panel and keeps its chat/repository navigation outside that scroll region.

## Connect an account

Install [Git](https://git-scm.com/downloads) and [GitHub CLI](https://github.com/cli/cli#installation), then run:

```sh
gh auth login --hostname github.com --scopes read:user
```

Refresh account activity in the Inspector after signing in. Lume reuses GitHub CLI authentication and does not send tokens to the webview or agents. Private content is limited by the account's permissions and scopes. This initial version supports github.com; GitHub Enterprise host selection is not implemented.

## Activity sources

The Inspector's local calendar uses a 90-day interval of commits reachable from the branch's HEAD, by all authors; narrow panels show the most recent whole weeks that fit. Its headline total covers that complete 90-day UTC interval, including when the repository returns only dates containing commits. The underlying repository history spans the last year. It is not GitHub's profile contribution count and does not establish which commits a chat produced. Shallow clones and the 20,000-commit history limit are disclosed in the interface.

The account calendar uses the official GitHub GraphQL `contributionsCollection`, including its daily contribution levels. Its headline total covers GitHub's returned contribution period, while the grid shows the recent whole weeks that fit the panel. Its expandable footer shows the three highest-ranked repositories from up to 10 returned by GitHub, ranked by commit contributions; those counts are not the same as the calendar's total of all contribution types. The supplied [Rare UI GitHub Activity](https://www.rareui.com/components/githubactivity) interaction is adapted to native Svelte and Lume's existing theme, with shared avatar crossfades and reduced-motion support. Repository avatars prioritize the [repository's custom Open Graph image](https://docs.github.com/en/graphql/reference/repos#repository), then fall back to the owner's avatar and finally initials if images are unavailable or fail to load. Cells stay at 11px, and the displayed date interval is explicit. Tooltips render in the viewport outside the scroll container. The expanding footer hides the covered calendar from keyboard and assistive-technology navigation.

## Boundaries of this first implementation

Git inspection works locally without GitHub authentication. Local observation is shared and polled every 15 seconds while its consumers are mounted and the window is visible; GitHub responses are cached for 60 seconds. Commands have deadlines and output limits. This is a read-only CLI transport, not the official GitHub MCP Server integration.

New PR and issue actions open GitHub's forms for explicit publication. Staging, committing, pushing, requesting review, merging and rerunning workflows are not automated. Native account connection, official MCP transport, scoped agent tools and approved writes through Lume's action broker remain under development.

A remote Node's repository must eventually be inspected through an RPC on its owning computer. The current commands inspect locally accessible directories and report unavailable ones; no remote repository RPC is implemented, and a repository is not guessed from a chat name.
