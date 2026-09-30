export function validateOrder(input) {
  const errors = [];
  if (!input || typeof input !== "object") errors.push("order must be an object");
  if (!input?.customerId) errors.push("customerId is required");
  if (!Array.isArray(input?.items) || input.items.length === 0) errors.push("at least one item is required");
  if (input?.items?.some((item) => !item.sku || !Number.isInteger(item.quantity) || item.quantity < 1)) {
    errors.push("every item needs a sku and positive integer quantity");
  }
  return { valid: errors.length === 0, errors };
}

export function orderTotal(items, catalog) {
  return items.reduce((sum, item) => sum + catalog[item.sku].price * item.quantity, 0);
}
