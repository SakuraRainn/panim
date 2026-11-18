# PANIM account proofs v1 (`panim/1`)

This specification gives the exact form of an account proof and of a revocation. Any tool can use it to make or examine proofs without PANIM. `docs/DESIGN.md` §6 tells the purpose of proofs.

A word in **bold** in a rule is a term of this specification. "Must" shows a rule. A block that does not obey a rule is not correct.

## 1. Terms

- **Persona key**: a raw secp256k1 key. Its address is the last 20 bytes of Keccak-256 of the 64-byte public key, as in `zikaron-v1.md` §5.5 of the ZIKARON repository.
- **Statement**: the five lines of §3. The persona key signs it.
- **Block**: the statement, a line feed and the `sig` line (§4). A person posts the block.
- **Account**: an account on a platform (§5).
- **Checker**: a person or a tool that examines a block.

## 2. Bytes

1. A statement and a block contain only the bytes 0x0A (line feed) and 0x20 to 0x7E (printable ASCII).
2. A line ends with one line feed (0x0A). The last line of a block has no line feed after it.
3. Two parts of a line have one space (0x20) between them. A line has no space at its start or its end.
4. All hexadecimal digits are lowercase.

## 3. The statement

A statement has five lines, in this sequence:

```
<domain>
persona <address>
platform <platform>
account <id> <handle>
date <date>
```

1. `<domain>` is `panim/1-proof` for a proof or `panim/1-revoke` for a revocation. No other value is correct.
2. `<address>` is the address of the persona key: `0x` and 40 lowercase hexadecimal digits.
3. `<platform>` is one name from the table in §5.
4. `<id>` is the stable identifier of the account on the platform, in decimal digits, with no zero at the start. If the platform has no stable identifier, `<id>` is `-`.
5. `<handle>` is the name of the account on the platform, in the form that §5 gives. If the account has no name, `<handle>` is `-`. `<id>` and `<handle>` cannot both be `-`.
6. `<date>` is a date in UTC, in the form `YYYY-MM-DD`.
   - For a proof, it is the date of the signature.
   - For a revocation, it is the date when the account stopped being part of the persona.

The statement does not contain the name of the persona, the display name of the account or a different account. It links only one account to one persona.

## 4. The signature

1. The **value** is SHA-256 of the bytes of the statement. These bytes start with the first byte of `<domain>` and stop after the last byte of `<date>`, with no line feed at the end.
2. The persona key signs the value under the domain of the first line. The message is `<domain> || 0x0A || hex32(value)`, and the digest is the EIP-191 digest of that message. The rules are the rules of `zikaron-v1.md` §5.2 to §5.7: RFC 6979 nonces, low-s, and `v` 27 or 28.
3. The `sig` line is `sig ` and the signature: `0x` and 130 lowercase hexadecimal digits, `r || s || v`.

The two domains are not ZIKARON domains. A signature under `panim/1-proof` is not a signature under `panim/1-revoke` or under a ZIKARON domain, because the domain literal is part of the message (`zikaron-v1.md` §5.6).

## 5. Platforms

| `<platform>` | `<id>` | `<handle>` |
|---|---|---|
| `github` | The numeric user id | The login, without `@` |
| `x` | The numeric user id | The username, without `@` |
| `telegram` | The numeric user id | The username without `@`, or `-` |
| `discord` | The numeric user id (snowflake) | The unique username |
| `email` | `-` | The address, in lowercase |

1. The id binds the proof to the account. A handle can change, so the handle is only a help for people.
2. For `email`, there is no id. The address binds the proof, so it must be in lowercase.
3. A future version can add platforms. A checker refuses a platform that it does not know.

## 6. How a person publishes a block

The account must publish the block itself. There are two methods:

1. **In the post.** The account posts the block as all of the text, or as part of the text, of a public post. Examples: a GitHub gist, an X post, a message in a public Telegram channel.
2. **By link.** The account publishes a link to a public document that contains the block. Use this method when the location is too short for the block. Examples: the bio of a Telegram account, the "About me" of a Discord account.

The block has 272 characters or fewer when `<id>` has 19 digits or fewer and `<handle>` has 15 characters or fewer. Thus, a block for a usual X account fits in one post of 280 characters.

`docs/DESIGN.md` §6 tells which location to use on each platform, and how strong each location is.

## 7. How a checker examines a block

Do these steps in sequence. If a step fails, the block is not a correct proof.

1. Get the text of the post or of the linked document.
2. Change each CR LF (0x0D 0x0A) to LF (0x0A). Some platforms and email change line ends.
3. Find a line that is `panim/1-proof` or `panim/1-revoke`. The block is this line and the five lines after it. The checker ignores the text before and after the block.
4. Make sure that the six lines obey §2, §3 and §4.3.
5. Calculate the value (§4.1).
6. Recover the signer from the signature, with the checks of `zikaron-v1.md` §5.4. Make sure that the signer is the `<address>` of the `persona` line.
7. Make sure that the account that published the post (§6) is the account of the `account` line:
   - If `<id>` is not `-`, compare the ids.
   - If `<id>` is `-`, compare the handles.
   - For the "by link" method, the account must publish the link, and the linked document must contain the block.
8. Make sure that the platform of the post is the `<platform>` of the block.

The date in a block is what the persona says. A checker that must know the time can use the time of the post on the platform.

## 8. Revocations

1. A revocation has the same form as a proof, with the domain `panim/1-revoke`.
2. For one persona and one account, the block with the latest date is the current state.
   - If the latest block is a proof, the account is part of the persona.
   - If the latest block is a revocation, the account is not part of the persona from that date.
   - If a proof and a revocation have the same date, the revocation is the current state.
3. A thief who takes an account does not have the persona key. Thus, the thief cannot make a revocation or a new proof.
4. The persona publishes a revocation on its other proven accounts, with the methods of §6. The revoked account can be in the hands of a thief, so do not use it.
   - A checker that examines a proof must also look for revocations on the other proven accounts of the persona.
   - A persona with only one proven account cannot publish a revocation in version 1.
5. If a thief has the persona key, the thief can sign proofs and revocations as the persona. Then nobody can tell which holder is the real owner. Version 1 has no protection against this.

## 9. Worked example

The private key is 32 bytes of `0x46`. This is the public test key of EIP-155. Do not use it for a real persona.

The address of the key is `0x9d8a62f656a8d1615c1294fd71e9cfb3e4855a4f`.

A proof for the GitHub account `octocat` (id 583231), signed on 2026-11-12:

```
panim/1-proof
persona 0x9d8a62f656a8d1615c1294fd71e9cfb3e4855a4f
platform github
account 583231 octocat
date 2026-11-12
sig 0xb566ce41a13d26ab1ed686f0f3032e296d6eb7953aa0ccef96065dbc33313ba771817564b371933a0cbaa3cda6cbe6a661e49eef56faddbf52033e3f5f389a2f1c
```

- The statement has 119 bytes.
- The value is `0x05e41222a4740f7c6789d1ea5535197a28f38a24a0c75b37d74c7672ed52c447`.

A revocation for the same account, from 2026-12-01:

```
panim/1-revoke
persona 0x9d8a62f656a8d1615c1294fd71e9cfb3e4855a4f
platform github
account 583231 octocat
date 2026-12-01
sig 0x13071f1ed3021cfefc6248699a3d60f97de86dfc4809dca953d1516a0b34dde672fa07cb713c2aa6b0b95e53786d712a93c1551c413e1feb68969a6f52affe9f1c
```

- The statement has 120 bytes.
- The value is `0x4ca526fe9de9d53a995cb1afa5be6d0f14ba2f4ea2d860eab5ffb7eb1c6feacc`.

## 10. Versions

1. This is version 1. The `1` in `panim/1-proof` and `panim/1-revoke` is the version.
   - Version 1 is temporary. The ZIKARON face law will replace it (`docs/DISCUSSION.md` §12).
   - A checker continues to examine version 1 blocks after the face law is ready.
2. A new version uses new domain literals. A checker refuses a domain that it does not know.
3. A new version can add platforms and lines. It does not change the meaning of a version 1 block.
