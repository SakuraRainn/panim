# PANIM account proofs v2 (`panim/2`)

Draft. This version is not published. It becomes final with release 0.3.0 (`docs/ROADMAP.md` §11).

This specification adds the platform `bluesky` to version 1. A Bluesky account has a DID as its stable identifier, not a number. Version 1 permits only decimal digits in `<id>`, so it cannot hold a DID (`docs/DISCUSSION.md` §43).

All rules of `panim-proof-v1.md` apply to version 2, except the changes in §2. The section numbers "v1 §n" refer to `panim-proof-v1.md`.

## 1. Terms

The terms of v1 §1 apply. This version adds these terms:

- **DID**: the decentralized identifier of a Bluesky account, for example `did:plc:z72i7hdynmk6r22z27h6tvur`. It stays the same when the handle changes, and when the account moves to a different server.
- **Repository**: the public data store of a Bluesky account. Each record in it has an address of the form `at://<DID>/<collection>/<key>`.

## 2. Changes from version 1

1. **The domains.** `<domain>` is `panim/2-proof` for a proof or `panim/2-revoke` for a revocation. These literals replace the literals of v1 §3.1 in a version 2 block.
2. **The id.** This rule replaces v1 §3.4:
   - For `bluesky`, `<id>` is the DID of the account, in the form of §3.
   - For all other platforms, `<id>` is the stable identifier of the account, in decimal digits, with no zero at the start. If the platform has no stable identifier, `<id>` is `-`.
3. **The platforms.** This table replaces the table of v1 §5:

| `<platform>` | `<id>` | `<handle>` |
|---|---|---|
| `github` | The numeric user id | The login, without `@` |
| `x` | The numeric user id | The username, without `@` |
| `telegram` | The numeric user id | The username without `@`, or `-` |
| `discord` | The numeric user id (snowflake) | The unique username |
| `email` | `-` | The address, in lowercase |
| `bluesky` | The DID (§3) | The handle without `@`, in lowercase, or `-` |

The rules of v1 §2 (bytes) and v1 §4 (signature) do not change. The signature uses the domain of the first line, so a version 2 signature is not a version 1 signature.

## 3. Bluesky

1. A DID in a block has one of these two forms:
   - `did:plc:` and 24 characters from `a` to `z` and from `2` to `7`.
   - `did:web:` and a host name in lowercase. The host name has only the letters `a` to `z`, the digits, `-` and `.`.
2. The DID must not have uppercase letters. Thus, two blocks for the same account have the same bytes in `<id>`.
3. A handle can change, so the handle is only a help for people (v1 §5.1). The DID binds the proof to the account.
4. A block fits in one post of 300 characters when the DID has the `did:plc:` form and the handle has 24 characters or fewer. The block then has 276 characters and the length of the handle.
   - For a longer handle, write `-` as the handle. The block then has 277 characters.
   - For a `did:web:` DID, the block can be too long. Use the "by link" method (v1 §6.2).

## 4. How a person publishes a Bluesky block

The methods of v1 §6 apply:

1. **In the post.** The account posts the block in a usual post (`app.bsky.feed.post`), and pins it. The block is all of the text of the post, or part of it.
2. **By link.** The account puts a link in its bio (the record `app.bsky.actor.profile`, key `self`). The link goes to a public document that contains the block.

The block must be in a usual post or in the bio. A record of a different type in the repository is not a correct location, because the Bluesky app does not show it to people.

## 5. How a checker examines a Bluesky block

Do the steps of v1 §7. For `bluesky`, do steps 1 and 7 as follows:

1. **Step 1.** Get the post record from the repository of the account:
   - A post URL has the form `https://bsky.app/profile/<handle or DID>/post/<key>`. If it has a handle, resolve the handle to a DID.
   - Get the record `at://<DID>/app.bsky.feed.post/<key>`, for example with `com.atproto.repo.getRecord`. The text of the post is the field `text`.
   - A checker can get the record from the server of the account, as the DID document gives it, or from a public index. The server of the account is the better source.
2. **Step 7.** Compare the DID of the repository that holds the post with the `<id>` of the block. For the "by link" method, the DID of the repository that holds the bio must be the `<id>`.

The checker does not need a login. All records in a repository are public.

## 6. Version 1 and version 2 together

1. A checker of version 2 also examines version 1 blocks, with the rules of version 1.
2. A version 1 block cannot have the platform `bluesky`. A checker refuses it.
3. For revocations (v1 §8), blocks of the two versions are one set when they have the same persona and the same account:
   - Two blocks have the same account when they have the same platform and the same `<id>`.
   - If `<id>` is `-`, they have the same account when they have the same `<handle>`.
   - Thus, a version 2 revocation can end a version 1 proof.
4. A tool that makes new blocks uses version 2 for `bluesky`. For the other platforms, it can use version 1 or version 2.

## 7. Worked example

To do in milestone 13: calculate a proof and a revocation for a Bluesky account with the test key of v1 §9. Calculate them with the code of `panim-core`, and keep them as test vectors. Give the statement, the number of bytes, the value and the `sig` line, as in v1 §9.

## 8. Versions

1. This is version 2. The `2` in `panim/2-proof` and `panim/2-revoke` is the version.
2. Version 2 is temporary, as version 1 is (v1 §10.1). The ZIKARON face law will replace it.
3. A new version uses new domain literals. It does not change the meaning of a version 1 or a version 2 block.
