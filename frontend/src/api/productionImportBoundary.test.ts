const sources = import.meta.glob<string>('/src/**/*.{ts,tsx}', {
  query: '?raw',
  import: 'default',
  eager: true,
})
const relativeImports = /(?:import|export)\s+(?:[^'";]+?\s+from\s+)?['"](\.[^'"]+)['"]/g

function normalize(path: string): string {
  const parts: string[] = []
  for (const part of path.split('/')) {
    if (part === '..') parts.pop()
    else if (part !== '.' && part !== '') parts.push(part)
  }
  return `/${parts.join('/')}`
}

function resolveImport(from: string, imported: string): string {
  const base = normalize(`${from.slice(0, from.lastIndexOf('/'))}/${imported}`)
  const candidates = [base, `${base}.ts`, `${base}.tsx`, `${base}/index.ts`, `${base}/index.tsx`]
  const resolved = candidates.find((candidate) => sources[candidate] != null)
  if (!resolved) throw new Error(`cannot resolve ${imported} from ${from}`)
  return resolved
}

function productionModules(): Map<string, string> {
  const modules = new Map<string, string>()

  function visit(path: string) {
    const source = sources[path]
    if (source == null) throw new Error(`missing source ${path}`)
    modules.set(path, source)
    for (const match of source.matchAll(relativeImports)) {
      const imported = match[1]
      if (imported.endsWith('.css')) continue
      const next = resolveImport(path, imported)
      if (!modules.has(next)) visit(next)
    }
  }

  visit('/src/main.tsx')
  return modules
}

it('keeps preview fixtures out of the production entry graph', () => {
  const modules = productionModules()
  expect([...modules.keys()]).not.toContain('/src/api/previewDesktopApi.ts')
  expect([...modules.values()].join('\n')).not.toMatch(/seedPreview|richPreviewStatus|echo-preview/)
})
