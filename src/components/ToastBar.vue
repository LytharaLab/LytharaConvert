<script setup>
import { state } from '../store.js';
</script>

<template>
  <div id="toast" class="toast" :class="{ show: state.toast.visible }" role="status">{{ state.toast.message }}</div>
</template>
