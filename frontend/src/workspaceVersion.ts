export function workspaceVersionFromManifest(manifest: string): string {
  const section = manifest.match(/\[workspace\.package\]([^[]*)/)
  const version = section?.[1]?.match(/^version\s*=\s*"([^"]+)"/m)
  if (!version) throw new Error('workspace.package.version not found in Cargo.toml')
  return version[1]
}
