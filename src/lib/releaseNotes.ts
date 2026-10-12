/** Drop the "### Assets" section (and anything after it): useful on GitHub, noise in the in-app updater. */
export function stripAssetsSection(body: string): string {
  return body.replace(/^#{1,6}\s*Assets\b[\s\S]*$/im, "").trim();
}
