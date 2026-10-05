export function validateName(name) {
  if (!name) return 'Name cannot be empty';
  if (!/^[a-z0-9]([a-z0-9-]*[a-z0-9])?$/.test(name)) return 'Lowercase alphanumeric + hyphens only (e.g. acme, client-b)';
  return true;
}

export function stripUrl(url) {
  try {
    const u = new URL(url);
    return `${u.protocol}//${u.host}`;
  } catch {
    return url.replace(/\/.*$/, '');
  }
}
