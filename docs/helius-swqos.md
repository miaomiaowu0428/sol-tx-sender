> ## Documentation Index
> Fetch the complete documentation index at: https://www.helius.dev/docs/llms.txt
> Use this file to discover all available pages before exploring further.

# Sender SWQOS-Only: Cost-Optimized Low-Latency Sending

> Send Solana transactions through Helius Sender's cost-optimized SWQOS-only path with a 0.000005 SOL minimum tip. Low latency at the lowest tip.

## Overview

SWQOS-only is the cost-optimized [Helius Sender](/docs/sending-transactions/sender) tier. It routes your transaction through a single fast SWQOS (stake-weighted quality of service) path with the **lowest minimum tip of any Sender tier: 0.000005 SOL**.

Use this tier when you want low-latency sending at the lowest tip. For highly contested transactions where the fastest landing matters, use the [Sender Max](/docs/sending-transactions/sender-max) tier instead.

<CardGroup cols={2}>
  <Card title="Lowest Tip" icon="dollar-sign">
    Minimum tip of just 0.000005 SOL — the cheapest way to use Sender
  </Card>

  <Card title="Single Fast Path" icon="bolt">
    Routes exclusively through SWQOS infrastructure for low-latency delivery
  </Card>

  <Card title="No Credits" icon="coins">
    Available on all plans without consuming API credits
  </Card>

  <Card title="Single Pathway" icon="circle-info">
    Routes through one SWQOS path only — for multi-path routing and the fastest
    landing, use the Sender Max tier
  </Card>
</CardGroup>

## When to use SWQOS-only

<CardGroup cols={2}>
  <Card title="Good fit" icon="circle-check">
    Cost-sensitive trading, high transaction volume, and flows where a single
    fast path is sufficient to land reliably.
  </Card>

  <Card title="Consider Sender Max instead" icon="trophy" href="/docs/sending-transactions/sender-max">
    Highly contested accounts, competitive launches, or any transaction where
    you need the fastest possible landing.
  </Card>
</CardGroup>

## How to enable SWQOS-only

Add the `swqos_only=true` query parameter to any Sender endpoint URL. Use the global HTTPS endpoint for frontends, or the regional HTTP endpoint closest to your servers for backends.

**Frontend / browser (global HTTPS):**

```
https://sender.helius-rpc.com/fast?swqos_only=true
```

**Backend / server (regional HTTP):**

```
http://slc-sender.helius-rpc.com/fast?swqos_only=true   # Salt Lake City
http://ewr-sender.helius-rpc.com/fast?swqos_only=true   # Newark
http://lon-sender.helius-rpc.com/fast?swqos_only=true   # London
http://fra-sender.helius-rpc.com/fast?swqos_only=true   # Frankfurt
http://ams-sender.helius-rpc.com/fast?swqos_only=true   # Amsterdam
http://sg-sender.helius-rpc.com/fast?swqos_only=true    # Singapore
http://tyo-sender.helius-rpc.com/fast?swqos_only=true   # Tokyo
```

This routes your transaction exclusively through SWQOS and applies the lower **0.000005 SOL** minimum tip requirement. See the [Sender endpoints reference](/docs/sending-transactions/sender#endpoints) for connection warming and details.

<Tip>
  Want to avoid validators linked to sandwich attacks? Add
  [`mev-protect=true`](/docs/sending-transactions/mev-protect) alongside `swqos_only`:
  `https://sender.helius-rpc.com/fast?swqos_only=true&mev-protect=true`.
</Tip>

## Requirements

Like every Sender tier, SWQOS-only transactions must include:

* **Tip**: A SOL transfer of at least **0.000005 SOL** to a designated [tip account](/docs/sending-transactions/sender#requirements)
* **Priority fee**: A compute unit price instruction via `ComputeBudgetProgram.setComputeUnitPrice`

Skipping preflight (`skipPreflight: true`) is optional but recommended — Preflight checks increase latency.

<Warning>
  Your transaction must contain both a tip transfer and a compute unit price
  instruction. Without both, it will be rejected.
</Warning>

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

    async function sendSwqosOnly(
      keypair: Keypair,
      recipientAddress: string
    ): Promise<string> {
      const connection = new Connection(
        `https://mainnet.helius-rpc.com/?api-key=${HELIUS_API_KEY}`
      );

      const { value: { blockhash } } = await connection.getLatestBlockhashAndContext('confirmed');

      // Build transaction: priority fee + recipient transfer + minimum SWQOS-only tip
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
              lamports: 0.000005 * LAMPORTS_PER_SOL, // SWQOS-only minimum tip
            })
          ],
          payerKey: keypair.publicKey,
          recentBlockhash: blockhash,
        }).compileToV0Message()
      );

      transaction.sign([keypair]);

      // Note the ?swqos_only=true query parameter
      const SENDER_ENDPOINT = 'https://sender.helius-rpc.com/fast?swqos_only=true';
      // Backend: use the regional HTTP endpoint closest to your servers
      // const SENDER_ENDPOINT = 'http://slc-sender.helius-rpc.com/fast?swqos_only=true';

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
    sendSwqosOnly(Keypair.fromSecretKey(bs58.decode(PRIV_B58)), RECIPIENT);
    ```
  </Tab>

  <Tab title="cURL">
    ```bash theme={"system"}
    curl "https://sender.helius-rpc.com/fast?swqos_only=true" \
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

<Note>
  The transaction above still uses a 0.000005 SOL tip — the SWQOS-only minimum.
  Tips between 0.000005 SOL and 0.001 SOL are accepted but stay on the single
  SWQOS path. To route across every pathway for the fastest landing, use the
  [Sender Max](/docs/sending-transactions/sender-max) tier with a minimum 0.001 SOL
  tip.
</Note>

## Best Practices

* **Endpoint selection**: Use `https://sender.helius-rpc.com/fast?swqos_only=true` for frontend apps to avoid CORS issues. For backend apps, use the regional HTTP endpoint closest to your servers.
* **Connection warming**: Use the [`/ping` endpoint](/docs/sending-transactions/sender#connection-warming) during idle periods longer than 5 seconds.
* **Priority fees**: Use the [Helius Priority Fee API](/docs/priority-fee-api) for real-time recommendations.
* **Retries**: Set `maxRetries: 0` and implement your own retry logic.

## Related

<CardGroup cols={2}>
  <Card title="Sender Overview" icon="rocket" href="/docs/sending-transactions/sender">
    Shared concepts: endpoints, connection warming, rate limits, and custom TPS.
  </Card>

  <Card title="Sender Max Tier" icon="trophy" href="/docs/sending-transactions/sender-max">
    The fastest landing via multi-path routing across all pathways (0.001 SOL min
    tip).
  </Card>

  <Card title="Transaction Rebates" icon="coins" href="/docs/sending-transactions/backrun-rebates">
    Opt in to earn a share of the MEV your transactions create, paid in SOL.
  </Card>

  <Card title="MEV Protect" icon="shield-halved" href="/docs/sending-transactions/mev-protect">
    Route around validators statistically linked to sandwich attacks.
  </Card>
</CardGroup>
