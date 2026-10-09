# Rusty OpenWiki brief

Build a durable engineering map of Rusty, the local-first assistant for Omarchy: a pure
MCP back end over a markdown vault, with a CLI and the `rusty` command; front ends such as
Marley, a Zed fork, draw over it. Organise the wiki around subsystems and end-to-end
workflows, never around the directory tree.

Prioritise:

- the vault as the truth and SQLite as a rebuildable index: page rules, frontmatter,
  the timeline section, wikilinks as vault paths, lenient pages, soft deletes;
- the `rusty-mcp` tool surface (tasks, notes, memories, brain, skills, secrets, settings,
  the workspace tools) and its two transports;
- the renderer to Qt rich text and the typed blocks beside it; the `rusty` command;
  export and import;
- semantic search and the embedding providers, and the setting that gates what leaves
  the machine;
- the workflow: the constitution, the gate and its receipts, the hooks, the planning
  record, CodeGraph at design and inspect, OpenWiki at complete;
- focused source and test anchors that help a future change find its owner and the
  narrowest verification path.

Keep roadmap intent apart from implemented behaviour. Name current limitations, the
product boundaries, and what is never sent off the machine. Nothing personal: no vault
pages, hostnames or accounts.
