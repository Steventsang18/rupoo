// Shared API helper — reads token from localStorage and includes it in all requests

function getToken() {
  return localStorage.getItem('rupoo_token') || '';
}

export function setToken(token) {
  localStorage.setItem('rupoo_token', token);
}

export function hasToken() {
  return !!getToken();
}

export async function api(url, options = {}) {
  const token = getToken();
  const headers = { ...options.headers };
  if (token) {
    headers['Authorization'] = `Bearer ${token}`;
  }
  const res = await fetch(url, { ...options, headers });
  if (res.status === 401) {
    // Token invalid/expired — clear it
    localStorage.removeItem('rupoo_token');
    throw new Error('unauthorized');
  }
  return res;
}

export function wsUrl(path) {
  const token = getToken();
  const sep = path.includes('?') ? '&' : '?';
  return `ws://127.0.0.1:8080${path}${sep}token=${token}`;
}
