// What each of Better Auth's roles may do, per 00012 D2. Web hides what a
// role cannot use; Auth and API refuse it regardless.

export type Role = "member" | "admin" | "owner";

// Better Auth stores several roles as one comma-separated string.
function roles(role: string) {
  return role.split(",").map((part) => part.trim());
}

export function isOwner(role: string) {
  return roles(role).includes("owner");
}

export function manages(role: string) {
  return roles(role).some((part) => part === "owner" || part === "admin");
}

/** The roles `role` may invite with or give to a member. */
export function assignableRoles(role: string): Role[] {
  if (isOwner(role)) return ["member", "admin", "owner"];
  if (manages(role)) return ["member", "admin"];
  return [];
}

/** Whether `role` may change or remove a member whose role is `target`. */
export function canChange(role: string, target: string) {
  return manages(role) && (isOwner(role) || !isOwner(target));
}

export function invitationLink(origin: string, invitationId: string) {
  return `${origin}/invite/${invitationId}`;
}

export function pendingInvitations<T extends { status: string; expiresAt: string | Date }>(
  invitations: T[],
  now: Date,
) {
  return invitations.filter(
    (invitation) =>
      invitation.status === "pending" && new Date(invitation.expiresAt).getTime() > now.getTime(),
  );
}
