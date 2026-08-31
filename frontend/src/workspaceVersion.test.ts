import { workspaceVersionFromManifest } from './workspaceVersion'

describe('workspaceVersionFromManifest', () => {
  it('does not confuse rust-version with the package version', () => {
    const manifest = `
[workspace.package]
rust-version = "1.88"
version = "0.14.0"

[workspace.dependencies]
serde = "1"
`
    expect(workspaceVersionFromManifest(manifest)).toBe('0.14.0')
  })
})
