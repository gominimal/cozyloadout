<!-- PR title: use a Conventional Commit subject — `type(scope): summary`,
     imperative, lower-case, no trailing period. The PR title becomes the
     squash commit subject. -->

## Summary

<!-- What changes, and why. Link related issues (`Refs: #123` / `Closes: #123`). -->

## Testing

<!-- What you ran. `just check` is the gate; paste the relevant output as
     evidence. If you changed a template or the renderer, say which schemes you
     rendered — `just check-schemes` covers the whole upstream collection. -->

## Checklist

- [ ] `just check` passes
- [ ] Templates edited under `templates/`, not `build/`
- [ ] AGENTS.md / README.md updated if behavior changed
