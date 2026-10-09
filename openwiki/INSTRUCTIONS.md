# Rusty OpenWiki brief

Build a durable engineering map of Rusty, a local-first memory and knowledge store for AI
agents on Omarchy: an MCP server over a markdown vault, with a CLI and the `rusty`
command; front ends such as Marley, a Zed fork, draw over it. Organise the wiki around subsystems and end-to-end
workflows, never around the directory tree.

Prioritise:

- the vault as the truth and SQLite as a rebuildable index: page rules, frontmatter,
  the timeline section, wikilinks as vault paths, lenient pages, soft deletes;
- the `rusty-mcp` tool surface (to-do lists, notes, memories, brain pages and search, the
  brain loop, bookmarks, sources, skills and scripts, secrets, settings, the change feed,
  the conversation archive) and its two transports; `docs/tools.md` lists the tools, so
  the wiki explains the rules a list does not show;
- the HTML renderer and the typed blocks beside it; the `rusty` command; export and
  import;
- semantic search and the embedding providers, and the setting that gates what leaves
  the machine;
- the workflow: the constitution, the gate and its receipts, the hooks, the planning
  record, CodeGraph at design and inspect, OpenWiki at complete;
- focused source and test anchors that help a future change find its owner and the
  narrowest verification path.

Keep roadmap intent apart from implemented behaviour. Name current limitations, the
product boundaries, and what is never sent off the machine. Nothing personal: no vault
pages, hostnames or accounts.
