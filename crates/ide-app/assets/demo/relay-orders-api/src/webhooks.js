const delivered = new Set();

export async function deliverWebhook(event, destination, send) {
  const key = `${destination}:${event.id}`;
  if (delivered.has(key)) return { delivered: false, reason: "duplicate" };
  await send(destination, event);
  delivered.add(key);
  return { delivered: true };
}

export function resetDeliveryMemory() {
  delivered.clear();
}
