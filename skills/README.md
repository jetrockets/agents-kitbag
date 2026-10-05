# Skills

Claude Code skills in `.skill` format are ZIP archives containing a folder with `SKILL.md` inside.

## Skill Catalog

| Skill | Description | Source | Pack | Unpack |
|-------|-------------|--------|------|--------|
| **Jira Task Creator** | Creates developer-ready Jira issues from business requirements | [SKILL.md](jira-task-creator-from-descriptions-as-a-project-manager/SKILL.md) | `zip -r jira-task-creator.skill jira-task-creator-from-descriptions-as-a-project-manager/` | `unzip -o jira-task-creator.skill` |
| **Asana Task Creator** | Creates developer-ready Asana tasks from business requirements | [SKILL.md](task-creator-from-descriptions-as-a-project-manager/SKILL.md) | `zip -r asana-task-creator.skill task-creator-from-descriptions-as-a-project-manager/` | `unzip -o asana-task-creator.skill` |
| **GitHub Task Creator** | Creates developer-ready GitHub issues in Projects from business requirements | [SKILL.md](github-task-creator-from-descriptions-as-a-project-manager/SKILL.md) | `zip -r github-task-creator.skill github-task-creator-from-descriptions-as-a-project-manager/` | `unzip -o github-task-creator.skill` |
| **Azure + Linear Task Creator** | Analyzes Azure DevOps repos, creates developer-ready Linear issues | [SKILL.md](azure-linear-task-creator-from-descriptions-as-a-project-manager/SKILL.md) | `zip -r azure-linear-task-creator.skill azure-linear-task-creator-from-descriptions-as-a-project-manager/` | `unzip -o azure-linear-task-creator.skill` |

## Structure

```
skills/
├── my-skill.skill                          # ZIP archive (ready to install)
└── my-skill-folder/                        # Unpacked folder
    └── SKILL.md                            # Skill content
```

## Pack (folder → .skill)

```bash
# Single skill
zip -r my-skill.skill my-skill-folder/

# All folders in current directory
for dir in */; do
  zip -r "${dir%/}.skill" "$dir"
done
```

## Unpack (.skill → folder)

```bash
# Single skill
unzip my-skill.skill

# All .skill files
for f in *.skill; do
  unzip -o "$f"
done
```
