> ## Documentation Index
> Fetch the complete documentation index at: https://www.helius.dev/docs/llms.txt
> Use this file to discover all available pages before exploring further.

# Sender Max: Fastest Solana Transaction Landing

> Send Solana transactions and bundles through Helius Sender's fastest-landing tier. Access all pathways (Helius, Jito, Harmonic, Rakurai, etc.) for every submission, with a 0.001 SOL minimum tip.

## Overview

<Note>
  **Pricing update:** Sender Max now uses a **0.001 SOL** minimum tip. This
  enters you into a priority tip buffer for top of block — tip more to land first.
</Note>

Sender Max is the highest-performance [Helius Sender](/docs/sending-transactions/sender) tier, built for submissions that need to land ahead of the competition. It routes your submission across every available high-speed pathway and enters it into a priority tip buffer that favors the highest tips.

Sender Max handles both **single transactions and bundles**. Both use all pathways (Helius, Jito, Harmonic, Rakurai, etc.) — the only difference is the payload: submit one transaction with `sendTransaction`, or up to four with `sendBundle`.

This tier requires a minimum tip of **0.001 SOL**. For cost-optimized sending on a single path, use the [SWQOS-only](/docs/sending-transactions/sender-swqos-only) tier instead.

<CardGroup cols={2}>
  <Card title="Fastest Landing" icon="trophy">
    Routed across every high-speed pathway for the highest probability of
    landing
  </Card>

  <Card title="Priority Tip Buffer" icon="arrow-up-wide-short">
    Your submission enters a priority tip buffer that prioritizes the
    highest-tipping submissions — tip more to land first
  </Card>

  <Card title="No Credits" icon="coins">
    Available on all plans without consuming API credits
  </Card>

  <Card title="0.001 SOL Minimum Tip" icon="dollar-sign">
    The minimum tip required to enter the priority tip buffer and use every
    routing pathway
  </Card>
</CardGroup>

## How it works

When you tip at least **0.001 SOL**, your submission — whether a single transaction or a bundle — is routed across every available high-speed pathway and entered into a priority tip buffer. The buffer favors the highest tips, so raising your tip above the minimum increases your chance of landing first.

<Note>
  Tips below 0.001 SOL do not enter the priority tip buffer — they are sent on a best-effort
  basis through fewer pathways. For predictable low-latency sending at a lower
  tip, use the [SWQOS-only](/docs/sending-transactions/sender-swqos-only) tier
  (0.000005 SOL).
</Note>

## When to use Max

<CardGroup cols={2}>
  <Card title="Good fit" icon="circle-check">
    Highly contested accounts, competitive token launches, liquidations, and any
    transaction where landing first is worth a higher tip.
  </Card>

  <Card title="Consider SWQOS-only instead" icon="dollar-sign" href="/docs/sending-transactions/sender-swqos-only">
    Cost-sensitive or high-volume flows where a single fast path lands reliably.
  </Card>
</CardGroup>

## Requirements

Every Sender Max submission must include:

* **Tip**: A SOL transfer of at least **0.001 SOL** to a designated [tip account](/docs/sending-transactions/sender#requirements). In a bundle, at least one transaction must carry the tip.
* **Priority fee**: A compute unit price instruction with a priority fee of at least **5,000 lamports**. See [Priority fees](#priority-fees).

Skipping preflight (`skipPreflight: true`) is optional but recommended — Preflight checks increase latency.

<Warning>
  Transactions missing the tip or priority fee will be rejected. For
  bundle-specific rules, see [Bundles](#bundles).
</Warning>

## Priority fees

Every transaction must include a priority fee of at least **5,000 lamports**, and we recommend **at least 10,000 lamports** for more reliable landing. The tip enters your transaction into the priority tip buffer, while the priority fee raises its priority in the validator's scheduler — together they maximize your chance of landing.

Set a priority fee with a compute unit price instruction. Your total priority fee is the compute unit price (in micro-lamports) multiplied by your compute unit limit:

```typescript theme={"system"}
import { ComputeBudgetProgram } from "@solana/web3.js";

ComputeBudgetProgram.setComputeUnitPrice({ microLamports: 200_000 });
```

Use the [Helius Priority Fee API](/docs/priority-fee-api) for real-time fee recommendations based on current network conditions.

## Endpoints

Use the global HTTPS endpoint for frontends, or the regional HTTP endpoint closest to your servers for backends.

**Frontend / browser (global HTTPS):**

```
https://sender.helius-rpc.com/fast
```

**Backend / server (regional HTTP):**

```
http://slc-sender.helius-rpc.com/fast   # Salt Lake City
http://ewr-sender.helius-rpc.com/fast   # Newark
http://lon-sender.helius-rpc.com/fast   # London
http://fra-sender.helius-rpc.com/fast   # Frankfurt
http://ams-sender.helius-rpc.com/fast   # Amsterdam
http://sg-sender.helius-rpc.com/fast    # Singapore
http://tyo-sender.helius-rpc.com/fast   # Tokyo
```

See the [Sender endpoints reference](/docs/sending-transactions/sender#endpoints) for connection warming and details.

<Tip>
  Want to avoid validators linked to sandwich attacks? Add
  [`mev-protect=true`](/docs/sending-transactions/mev-protect) to the endpoint URL —
  it works for both `sendTransaction` and `sendBundle`:
  `https://sender.helius-rpc.com/fast?mev-protect=true`.
</Tip>

## Code Example

<Tabs>
  <Tab title="@solana/web3.js">
    ```typescript [expandable] theme={"system"}
    import { 
      Connection, 
      TransactionMessage,
      VersionedTransaction,
      SystemProgram, 
      PublicKey,
      Keypair,
      LAMPORTS_PER_SOL,
      ComputeBudgetProgram
    } from '@solana/web3.js';
    import bs58 from 'bs58';

    const PRIV_B58 = 'Your Private Key';
    const RECIPIENT = 'Recipient Address';
    const HELIUS_API_KEY = 'Your API Key';
    const TIP_ACCOUNTS = [
      "4ACfpUFoaSD9bfPdeu6DBt89gB6ENTeHBXCAi87NhDEE",
      "D2L6yPZ2FmmmTKPgzaMKdhu6EWZcTpLy1Vhx8uvZe7NZ",
      "9bnz4RShgq1hAnLnZbP8kbgBg1kEmcJBYQq3gQbmnSta",
      "5VY91ws6B2hMmBFRsXkoAAdsPHBJwRfBht4DXox3xkwn",
      "2nyhqdwKcJZR2vcqCyrYsaPVdAnFoJjiksCXJ7hfEYgD",
      "2q5pghRs6arqVjRvT5gfgWfWcHWmw1ZuCzphgd5KfWGJ",
      "wyvPkWjVZz1M8fHQnMMCDTQDbkManefNNhweYk5WkcF",
      "3KCKozbAaF75qEU33jtzozcJ29yJuaLJTy2jFdzUY8bT",
      "4vieeGHPYPG2MmyPRcYjdiDmmhN3ww7hsFNap8pVN3Ey",
      "4TQLFNWK8AovT1gFvda5jfw2oJeRMKEmw7aH6MGBJ3or"
    ];

    async function sendWithSenderMax(
      keypair: Keypair,
      recipientAddress: string
    ): Promise<string> {
      const connection = new Connection(
        `https://mainnet.helius-rpc.com/?api-key=${HELIUS_API_KEY}`
      );

      const { value: { blockhash } } = await connection.getLatestBlockhashAndContext('confirmed');

      // Build transaction: priority fee + recipient transfer + 0.001 SOL tip
      const transaction = new VersionedTransaction(
        new TransactionMessage({
          instructions: [
            ComputeBudgetProgram.setComputeUnitLimit({ units: 100_000 }),
            ComputeBudgetProgram.setComputeUnitPrice({ microLamports: 200_000 }),
            SystemProgram.transfer({
              fromPubkey: keypair.publicKey,
              toPubkey: new PublicKey(recipientAddress),
              lamports: 0.001 * LAMPORTS_PER_SOL,
            }),
            SystemProgram.transfer({
              fromPubkey: keypair.publicKey,
              toPubkey: new PublicKey(TIP_ACCOUNTS[Math.floor(Math.random() * TIP_ACCOUNTS.length)]),
              lamports: 0.001 * LAMPORTS_PER_SOL, // Minimum tip to enter the priority tip buffer
            })
          ],
          payerKey: keypair.publicKey,
          recentBlockhash: blockhash,
        }).compileToV0Message()
      );

      transaction.sign([keypair]);

      // Frontend: use the global HTTPS endpoint to avoid CORS issues
      const SENDER_ENDPOINT = 'https://sender.helius-rpc.com/fast';
      // Backend: use the regional HTTP endpoint closest to your servers
      // const SENDER_ENDPOINT = 'http://slc-sender.helius-rpc.com/fast';

      const response = await fetch(SENDER_ENDPOINT, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          jsonrpc: '2.0',
          id: Date.now().toString(),
          method: 'sendTransaction',
          params: [
            Buffer.from(transaction.serialize()).toString('base64'),
            {
              encoding: 'base64',
              skipPreflight: true, // Optional: skip for lower latency
              maxRetries: 0
            }
          ]
        })
      });

      const json = await response.json();
      if (json.error) {
        throw new Error(json.error.message);
      }

      console.log('Transaction sent:', json.result);
      return json.result;
    }

    // Usage
    sendWithSenderMax(Keypair.fromSecretKey(bs58.decode(PRIV_B58)), RECIPIENT);
    ```
  </Tab>

  <Tab title="cURL">
    ```bash theme={"system"}
    curl "https://sender.helius-rpc.com/fast" \
      -X POST \
      -H "Content-Type: application/json" \
      -d '{
        "jsonrpc": "2.0",
        "id": 1,
        "method": "sendTransaction",
        "params": [
          "BASE64_ENCODED_TRANSACTION",
          {
            "encoding": "base64",
            "skipPreflight": true,
            "maxRetries": 0
          }
        ]
      }'
    ```
  </Tab>
</Tabs>

<Tip>
  For competitive transactions, the 0.001 SOL minimum is unlikely to be
  competitive. Price your tip closer to the value of the transaction you're
  trying to land — the more a successful landing is worth to you, the higher you
  should tip. The minimum is a floor, not a target.
</Tip>

## Bundles

A bundle is just another payload Sender Max accepts. Submit up to 4 fully-signed transactions and they execute atomically (all-or-nothing) in order, entered into the **same priority tip buffer** as a single transaction — the buffer prioritizes the highest-tipping bundles for the fastest landing.

<Warning>
  **Provide only the 0.001 SOL Sender tip.** Sender Max routes your bundle
  across every high-speed pathway and adds any pathway tips on your behalf — you
  don't add a separate Jito tip, use Jito tip accounts, or set the `jito-region`
  header. Those apply only to [basic
  bundles](/docs/sending-transactions/send-bundle).
</Warning>

### Requirements

* **Up to 4 transactions**, fully signed. They execute sequentially and atomically — if any fails, the whole bundle is rejected.
* **Tip**: at least one transaction must transfer at least **0.001 SOL** to a designated [Sender tip account](/docs/sending-transactions/sender#requirements). This is the same Sender tip used for single transactions — Helius adds any pathway tips (including Jito) for you, so you don't add a separate Jito tip.
* **Priority fee**: each transaction must include a [priority fee](#priority-fees) of at least **5,000 lamports**.

### Submit

Send to the Sender endpoint with `method: "sendBundle"`. Use the global HTTPS endpoint for frontends, or any [regional endpoint](#endpoints) for backends — all Sender regions are supported.

<Tabs>
  <Tab title="@solana/web3.js">
    ```typescript [expandable] theme={"system"}
    import { 
      Connection, 
      TransactionMessage,
      VersionedTransaction,
      SystemProgram, 
      PublicKey,
      Keypair,
      LAMPORTS_PER_SOL,
      ComputeBudgetProgram
    } from '@solana/web3.js';
    import bs58 from 'bs58';

    const PRIV_B58 = 'Your Private Key';
    const HELIUS_API_KEY = 'Your API Key';
    const TIP_ACCOUNTS = [
      "4ACfpUFoaSD9bfPdeu6DBt89gB6ENTeHBXCAi87NhDEE",
      "D2L6yPZ2FmmmTKPgzaMKdhu6EWZcTpLy1Vhx8uvZe7NZ",
      "9bnz4RShgq1hAnLnZbP8kbgBg1kEmcJBYQq3gQbmnSta",
      "5VY91ws6B2hMmBFRsXkoAAdsPHBJwRfBht4DXox3xkwn",
      "2nyhqdwKcJZR2vcqCyrYsaPVdAnFoJjiksCXJ7hfEYgD",
      "2q5pghRs6arqVjRvT5gfgWfWcHWmw1ZuCzphgd5KfWGJ",
      "wyvPkWjVZz1M8fHQnMMCDTQDbkManefNNhweYk5WkcF",
      "3KCKozbAaF75qEU33jtzozcJ29yJuaLJTy2jFdzUY8bT",
      "4vieeGHPYPG2MmyPRcYjdiDmmhN3ww7hsFNap8pVN3Ey",
      "4TQLFNWK8AovT1gFvda5jfw2oJeRMKEmw7aH6MGBJ3or"
    ];

    async function sendBundleWithSenderMax(keypair: Keypair): Promise<string[]> {
      const connection = new Connection(
        `https://mainnet.helius-rpc.com/?api-key=${HELIUS_API_KEY}`
      );

      const { value: { blockhash } } = await connection.getLatestBlockhashAndContext('confirmed');

      const tipAccount = new PublicKey(TIP_ACCOUNTS[Math.floor(Math.random() * TIP_ACCOUNTS.length)]);

      // Transaction 1: your instructions + priority fee + the 0.001 SOL Sender tip
      const tx1 = new VersionedTransaction(
        new TransactionMessage({
          instructions: [
            ComputeBudgetProgram.setComputeUnitLimit({ units: 100_000 }),
            ComputeBudgetProgram.setComputeUnitPrice({ microLamports: 200_000 }),
            // ... your instructions here ...
            SystemProgram.transfer({
              fromPubkey: keypair.publicKey,
              toPubkey: tipAccount,
              lamports: 0.001 * LAMPORTS_PER_SOL, // Sender tip — only one tx in the bundle needs it
            })
          ],
          payerKey: keypair.publicKey,
          recentBlockhash: blockhash,
        }).compileToV0Message()
      );
      tx1.sign([keypair]);

      // Transaction 2: more instructions — executes after tx1, atomically
      const tx2 = new VersionedTransaction(
        new TransactionMessage({
          instructions: [
            ComputeBudgetProgram.setComputeUnitPrice({ microLamports: 200_000 }),
            // ... your instructions here ...
          ],
          payerKey: keypair.publicKey,
          recentBlockhash: blockhash,
        }).compileToV0Message()
      );
      tx2.sign([keypair]);

      // Serialize each fully-signed transaction (up to 4) to base64
      const bundle = [tx1, tx2].map((tx) => Buffer.from(tx.serialize()).toString('base64'));

      const response = await fetch('https://sender.helius-rpc.com/fast', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          jsonrpc: '2.0',
          id: Date.now().toString(),
          method: 'sendBundle',
          params: [bundle, { encoding: 'base64' }]
        })
      });

      const json = await response.json();
      if (json.error) {
        throw new Error(json.error.message);
      }

      // Track landing by each transaction's signature (see below)
      const signatures = [tx1, tx2].map((tx) => bs58.encode(tx.signatures[0]));
      console.log('Bundle submitted:', signatures);
      return signatures;
    }

    // Usage
    sendBundleWithSenderMax(Keypair.fromSecretKey(bs58.decode(PRIV_B58)));
    ```
  </Tab>

  <Tab title="cURL">
    ```bash theme={"system"}
    curl "https://sender.helius-rpc.com/fast" \
      -X POST \
      -H "Content-Type: application/json" \
      -d '{
        "jsonrpc": "2.0",
        "id": 1,
        "method": "sendBundle",
        "params": [
          [
            "BASE64_SIGNED_TX_1",
            "BASE64_SIGNED_TX_2"
          ],
          { "encoding": "base64" }
        ]
      }'
    ```
  </Tab>
</Tabs>

### Track landing

Track a bundle the same way you track a single transaction — by watching for each transaction's signature to land. Because the bundle is atomic, confirming any one transaction means the whole bundle landed. You do **not** use `getBundleStatuses` or a bundle ID.

Use whichever method fits your stack:

* Poll [`getSignatureStatuses`](/docs/api-reference/rpc/http/getsignaturestatuses) until the signature confirms.
* Subscribe to [LaserStream](/docs/laserstream) or [LaserStream WebSocket](/docs/rpc/websocket) and watch for the landed signature.
* Use [Shred Delivery](/docs/shred-delivery/raw-shreds) or any other Helius streaming product that exposes landed signatures.

## Best Practices

* **Endpoint selection**: Use `https://sender.helius-rpc.com/fast` for frontend apps to avoid CORS issues. For backend apps, use the regional HTTP endpoint closest to your servers.
* **Dynamic tipping**: Raise your tip above 0.001 SOL for highly contested transactions to improve your priority in the tip buffer.
* **Connection warming**: Use the [`/ping` endpoint](/docs/sending-transactions/sender#connection-warming) during idle periods longer than 5 seconds.
* **Priority fees**: Use the [Helius Priority Fee API](/docs/priority-fee-api) for real-time recommendations.
* **Retries**: Set `maxRetries: 0` and implement your own retry logic.

## Related

<CardGroup cols={2}>
  <Card title="Sender Overview" icon="rocket" href="/docs/sending-transactions/sender">
    Shared concepts: endpoints, connection warming, rate limits, and custom TPS.
  </Card>

  <Card title="SWQOS-Only Tier" icon="dollar-sign" href="/docs/sending-transactions/sender-swqos-only">
    Cost-optimized low-latency sending on a single path (0.000005 SOL min tip).
  </Card>

  <Card title="MEV Protect" icon="shield-halved" href="/docs/sending-transactions/mev-protect">
    Route around validators statistically linked to sandwich attacks.
  </Card>
</CardGroup>
