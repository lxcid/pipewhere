import assert from "node:assert/strict";
import { describe, test } from "node:test";

import {
  assignableRoles,
  canChange,
  invitationLink,
  isOwner,
  manages,
  pendingInvitations,
} from "./roles.ts";

describe("roles", () => {
  test("owners and admins manage; members do not", () => {
    assert.equal(manages("owner"), true);
    assert.equal(manages("admin"), true);
    assert.equal(manages("member"), false);
  });

  test("a comma-separated role counts each of its roles", () => {
    assert.equal(manages("member,admin"), true);
    assert.equal(isOwner("admin, owner"), true);
    assert.equal(isOwner("admin"), false);
  });

  test("only an owner may grant the owner role", () => {
    assert.deepEqual(assignableRoles("owner"), ["member", "admin", "owner"]);
    assert.deepEqual(assignableRoles("admin"), ["member", "admin"]);
    assert.deepEqual(assignableRoles("member"), []);
  });

  test("an admin may change a member or admin, but not an owner", () => {
    assert.equal(canChange("admin", "member"), true);
    assert.equal(canChange("admin", "admin"), true);
    assert.equal(canChange("admin", "owner"), false);
    assert.equal(canChange("owner", "owner"), true);
    assert.equal(canChange("member", "member"), false);
  });
});

describe("invitations", () => {
  test("the link opens Web's invitation page", () => {
    assert.equal(
      invitationLink("http://localhost:3000", "inv_01"),
      "http://localhost:3000/invite/inv_01",
    );
  });

  test("only pending invitations that have not expired are listed", () => {
    const now = new Date("2026-11-21T12:00:00Z");
    const invitations = [
      { id: "a", status: "pending", expiresAt: "2026-11-22T12:00:00Z" },
      { id: "b", status: "pending", expiresAt: "2026-11-21T12:00:00Z" },
      { id: "c", status: "accepted", expiresAt: "2026-11-22T12:00:00Z" },
      { id: "d", status: "canceled", expiresAt: "2026-11-22T12:00:00Z" },
    ];
    assert.deepEqual(
      pendingInvitations(invitations, now).map((invitation) => invitation.id),
      ["a"],
    );
  });
});
