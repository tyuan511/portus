# Bulwark Frontend Engineering Standards

1. **Package Manager**: Always use `pnpm` (never `npm` or `yarn`) for all frontend packages in `apps/desktop` (and future `apps/web`).
2. **shadcn/ui Components (Base UI + Nova)**:
   - All UI primitives in `src/components/ui/` MUST be installed via the `shadcn` CLI:
     ```bash
     pnpm dlx shadcn@latest add <component> -y
     ```
   - Never manually author or copy-paste `src/components/ui/*` files.
   - The project uses the `base-nova` preset (`@base-ui/react` + Nova theme).
3. **Component Size Limit**:
   - **Strictly forbid React components exceeding 1000 lines** in a single `.tsx` file.
   - Keep individual views, dialogs, and inspector panels modular (ideally under 300 lines per file) under `src/components/bulwark/`.
