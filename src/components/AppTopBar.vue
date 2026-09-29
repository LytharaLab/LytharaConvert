<script setup>
import { computed } from 'vue';
import { state, config, effectiveTheme, toggleTheme } from '../store.js';
import AppIcon from './AppIcon.vue';

const dark = computed(() => effectiveTheme() === 'dark');
const themeLabel = computed(() => (dark.value ? '切换到浅色模式' : '切换到深色模式'));
const engineLabel = computed(() => (state.engine.ready ? '转换引擎就绪' : 'FFmpeg 未就绪'));
</script>

<template>
  <header class="topbar">
    <div class="breadcrumb">工作台 <span>/</span> <b>{{ config.label }}</b></div>
    <div class="top-actions">
      <div class="status-pill" :class="{ warning: !state.engine }">
        <span class="status-dot"></span>{{ engineLabel }}
      </div>
      <button class="icon-button" :title="themeLabel" :aria-label="themeLabel" @click="toggleTheme">
        <AppIcon :name="dark ? 'sun' : 'moon'" />
      </button>
      <button class="icon-button" title="偏好设置" aria-label="偏好设置" @click="state.settingsOpen = true">
        <AppIcon name="settings" />
      </button>
    </div>
  </header>
</template>
