# PANIM: design

This document tells what PANIM is and what it does. It shows the current agreement. `ARCHITECTURE.md` tells how we build PANIM. `DISCUSSION.md` is the record of the decisions.

## 1. What PANIM is

The name PANIM (פנים) means "face". In Hebrew, this word is always plural. Thus, one person has faces. PANIM is a persona manager. One person uses several personas, and each persona has its own accounts.

PANIM has two functions:

1. **A product.** PANIM is a desktop app. A person connects their accounts to it. Then the person reads and answers all updates in one location.
2. **A layer.** Later, the persona layer of PANIM becomes a family component. ZIKARON, GALEED and ERAVON will use it to find which persona does an action.

## 2. Principles

- **Local, not hosted.** Only PANIM knows which personas are one person. This data stays on the computer of the person. There is no PANIM server.
- **Each person starts with a persona.** Each account belongs to a persona. A new person has one persona. The person can add more personas. In step 1, this one persona is the full person (§3).
- **The person selects the links.** By default, the personas of a person have no link between them. The person selects which personas to show as one person, and to whom.
- **A persona has a key.** Each persona has its own signing key. Other people know the persona by this key, not by the person. Each persona has a different key. If two personas use the same key, the key links them. A persona shows which accounts belong to it with account proofs (§6).
- **A persona is easy to make. Trust comes from work.** Any person can make a new persona at no cost. A new persona has no value. A persona gets trust slowly, from its work and its actions over time. One scandal or one scam can end the trust in a persona.
- **Only a persona judges, and only a persona is judged.** There is no level above the personas. A judgement stays with the persona that it names. It does not go to the person, or to the other personas of the person. The person can stop using a judged persona and make a new persona. The new persona starts with no history.
- **Personas stay apart.** Each persona has its accounts, one browser profile (cookies and storage) and one network route. Thus, services cannot link personas by IP address or by cookies. The accounts of one persona use the same profile and route, because they are one face.
- **Obey the rules of the platforms.** PANIM does not scrape pages. It does not control a user account through an API that is for bots. If an API does not give access, PANIM shows the account in the browser profile of the persona.

Known limit: Different profiles and routes stop easy links between personas. They do not stop a tracker that tries hard. All profiles use the same web engine on the same computer, with the same fonts, screen and graphics. A tracker can use this fingerprint to link the personas. Spam tools give each profile a false fingerprint, so we do not do that. A different method makes all profiles look the same, as Tor Browser does. We will examine this method later.

## 3. Steps

1. **An account manager for one full person.**
   - The person reads and answers the accounts on the step 1 platforms (§4).
   - PANIM sees the person as one full person. There is one persona. The user interface does not show it or divide it.
   - We prepare for step 2. Each account, the browser profile and the network route belong to this persona. Thus, a second persona does not change the stored data.
   - The persona has its signing key from the start, and it can make account proofs (§6).
2. **Several personas.**
   - The person adds personas, and each persona stays apart.
   - The person can show that two personas are one person. First, the person signs with the two keys. Later, the person uses a zero-knowledge proof.
3. **Trust.**
   - The key of each persona becomes its ZIKARON identity.
   - ZIKARON anchors the history of each persona.
   - GALEED and ERAVON use this layer.

## 4. Step 1 platforms

PANIM does not support the Meta services (Facebook, Instagram, Threads and WhatsApp). It will not support them in the future.

| Platform | How PANIM shows it | Notes |
|---|---|---|
| GitHub | In the shared timeline | The API is free. PANIM uses notifications and the events of the repositories that the person watches. |
| Telegram | In the shared timeline: one item for each chat. A chat panel shows the text messages and sends text. | TDLib gives full access. Muted and archived chats stay out. The read state follows Telegram in the two directions. Each account must have a phone number. Telegram knows the number, so a number can link personas. |
| Proton Mail | Paid plans: in the shared timeline. Free plans: in a browser view. | For paid plans, PANIM uses Proton Mail Bridge, a local IMAP/SMTP server. Later, the same code will serve other email services. |
| X | In a browser view always. Also in the shared timeline if the user adds their own API key. | The API is pay-per-use, and X bills the key of the user: $0.005 for each post read, $0.015 for each post, $0.20 for each post with a link. |
| Discord | In a browser view only | The API is for bots. A self-bot breaks the terms of Discord. |

The recommended order: GitHub, Telegram, Proton Mail, X, Discord.

Release 0.3.0 adds two platforms (`DISCUSSION.md` §42):

| Platform | How PANIM shows it | Notes |
|---|---|---|
| Bluesky | In the shared timeline | The AT Protocol is an open API for clients. The stable id of an account is its DID. |
| Email services | In the shared timeline | IMAP/SMTP with a password or an app password, for example Fastmail, mailbox.org, iCloud, Yahoo and Gmail. Proton Mail Bridge uses the same connector. Tuta has no IMAP, so it stays in a browser view. |

## 5. Two types of account

- **Joined accounts** send their updates to the shared timeline. PANIM reads the updates and keeps them on the computer. The person searches and answers them in PANIM.
- **Browser-view accounts** open the web app of the service in the browser profile of the persona. The profile keeps the login. PANIM shows these accounts, but it does not read their content. A small script reads only the unread count of the page (§7). The profile is not only for the step 1 platforms. The persona can keep a login on any site.

## 6. Persona keys and account proofs

Each persona has its own signing key from step 1. In step 3, the same key becomes the ZIKARON identity of the persona. Thus, the signatures that the persona made before step 3 stay with the persona.

An **account proof** shows that the holder of the key of a persona also controls an account:

1. The key of the persona signs a short statement. The statement gives the persona, the platform, the account and the date.
2. The person posts the statement from the account, in a public location on the platform. The person posts it, not PANIM. Thus, the proof obeys the rules of the platform.
3. Any person can make sure that the signature is from the persona, and that the post is from the account.

What a proof does:

- **It stops impersonation.** An account can say that it is a persona. If the account has no proof from the key of that persona, it is not that persona.
- **It lets a persona keep its record on all platforms.** The actions of the persona on each proven account belong to the persona, not to the platform. The record does not show the person.

What a proof does not do:

- **It does not show that the persona is honest.** A new persona has correct proofs but no history. Trust comes from the actions of the persona after the proof. From step 3, trust also comes from the anchored history.
- **It does not protect an account that a thief takes.** The thief does not have the key, so the thief cannot sign new statements. The persona signs a revocation of the account and publishes it on its other proven accounts.
- **It does not protect a stolen persona key.** The thief can then sign proofs and revocations as the persona. Nobody can then tell which holder is the real owner. Keep the persona key safe.

Proofs are public. Thus, any person can see the link between the accounts of one persona. This is correct, because these accounts are one face. The key of the persona that has the account always signs the proof. Thus, the proof of one persona never shows on the account of a different persona.

The proof must stay in a public location on the platform. A more public platform gives a stronger proof:

| Platform | Location of the proof | Strength |
|---|---|---|
| GitHub | A public gist, or the README of the profile | Strong. It is public and it stays. The persona key can also sign commits. |
| X | A pinned post, or a link in the bio | Strong while the post stays |
| Telegram | A message in a public channel, or the bio | Good. The bio is short, so it can hold only a link to the statement. |
| Discord | "About me", or a message in a server | Weak. Usually, only the people in the same server see the profile. |
| Bluesky (release 0.3.0) | A pinned post, or a link in the bio | Strong. Each post is a public record in the signed repository of the account. A post cannot change after it is published. |
| Email | A signed message, or a DNS record for a domain that the person has | On a shared service, the proof is only for the person who gets the message. With a domain of the person, the proof is for all people. |

Known limit: Before step 3, a persona cannot replace a lost or stolen key and keep its history. The persona must start again with a new key. PANIM seals the key with the other secrets when the key is not in use.

## 7. What the person sees (outline)

- The accounts of the person and their unread state. From step 2, PANIM puts the accounts into groups by persona.
  - For a browser-view account, a small script in the page reads the unread count from the title or the icon of the page. It also catches the notifications of the page. It sends only the count and the notice to PANIM, not the content of the messages.
- One timeline for all the joined accounts, with a filter by account. From step 2, there is also a filter by persona.
  - A Telegram chat is one item. It shows the last message and the unread count of the chat. The count of a Telegram account is the number of chats with unread messages.
  - A chat panel shows the last text messages of a Telegram chat. The person can send a text message from it.
- A function to write and answer from the timeline, as the selected account.
- Browser windows that open in the profile of the persona.
- A function to make an account proof. PANIM signs the statement. Then it shows the text to post, and where to post it.
- A function to examine the proofs of a different persona.
- From step 2: a function to select which personas to show as linked, and to whom.
- One passcode to open the app. It has eight characters or more. PANIM seals the credentials and the browser profiles when they are not in use.
  - After five wrong tries, the gate locks. Then only the key file opens the app, and the person sets a new passcode.
  - The app locks after 15 minutes without input. The person can change this time.
- The 12 recovery words of the persona. The persona key comes from them. The person writes them on paper and keeps them outside PANIM. They reset a lost passcode.
  - On the first start, PANIM shows the 12 words. Then it asks for four of them. The first start is complete only after this check.
  - In the settings, the person can see the words again with the passcode.
  - Each persona has its own 12 words. Thus, the words of one persona do not show or link a different persona.
- A key file of the persona key, if the person wants an extra copy. It has its own password. It also resets a lost passcode.
- The Telegram app values. Telegram knows each app by its `api_id`. A release of PANIM has the values of PANIM, and all the people who use PANIM share them. In the settings, the person can select their own values for new logins. Each account keeps the values of its login.
  - An `api_id` belongs to one Telegram account. If the logins of several personas use the same own `api_id`, Telegram can link the personas. The settings tell this.
  - A build from the source has no values of PANIM. Then the person must give their own values.
- A backup of the vault in one file. The file stays sealed. On the first start, the person can restore a backup instead of a new start.
  - The backup has the browser logins and the Telegram sessions. If PANIM still runs on the old computer, the same sessions are then in two places. The restore screen tells the person to close PANIM there first.
- The interface is in English and Chinese. The person selects the language.

In step 1, there is one browser profile. Thus, each site sees one login. For a second X or Discord account in a browser view, use the account switcher of the service. In step 2, the second account can go to a different persona.

## 8. Open questions

- How does a person show a link between personas: to one party or to all people? Can the person cancel the link after other people see it?
- Can a persona use its profile in the browser of the person (Chromium `--user-data-dir`, Firefox `--profile`) for usual browsing? Then the profile is not sealed by PANIM.
- How does work give trust to a persona? Which personas can accept work, and how much trust does each acceptance give (step 3)?
- How does each persona get gas from a different source (step 3)?
- A person finds one proven account of a persona. How does the person find all the other proven accounts? Is there a signed list in each proof, or in a different location? A checker must also find the revocations on these accounts (§6).
