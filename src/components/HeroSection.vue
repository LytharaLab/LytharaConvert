<script setup>
import { computed } from 'vue';
import { config, state } from '../store.js';

const title = computed(() => config.value.title);
const description = computed(() => config.value.desc);
</script>

<template>
  <section class="hero">
    <div class="hero-copy">
      <span class="eyebrow"><span class="sparkle">✦</span> YOUR MEDIA, YOUR WAY</span>
      <h1 v-html="title"></h1>
      <p v-html="description"></p>
      <div class="hero-badges">
        <span>◈ 批量处理</span><span>◇ 完全离线</span><span>◎ 自由格式</span>
      </div>
    </div>
    <div class="hero-art" aria-hidden="true">
      <div class="orbit orbit-one"></div>
      <div class="orbit orbit-two"></div>
      <div class="media-chip chip-png">PNG <small>IMAGE</small></div>
      <div class="media-chip chip-mp4">MP4 <small>VIDEO</small></div>
      <div class="media-chip chip-mp3">MP3 <small>AUDIO</small></div>
      <div class="center-disc"><img src="../assets/icon.png" alt="" /></div>
      <div class="orbit-spark spark-a"></div>
      <div class="orbit-spark spark-b"></div>
      <div v-if="state.transfer.active" class="transfer-badge">
        {{ state.transfer.note }} {{ state.transfer.progress }}%
      </div>
    </div>
  </section>
</template>
