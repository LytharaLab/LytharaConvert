<script setup>
import { symbols } from '../icons.js';

defineProps({
  name: { type: String, required: true }
});
</script>

<template>
  <svg
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    stroke-width="1.9"
    stroke-linecap="round"
    stroke-linejoin="round"
    aria-hidden="true"
    v-html="symbols[name] || ''"
  />
</template>
