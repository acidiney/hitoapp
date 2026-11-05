export function accountName(user: {
  username: string;
  display_name?: string | null;
}): string {
  return user.display_name || user.username;
}
