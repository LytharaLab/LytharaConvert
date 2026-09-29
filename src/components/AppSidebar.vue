<script setup>
import { state, setMode } from '../store.js';
import AppIcon from './AppIcon.vue';

const items = [
  { mode: 'convert', icon: 'swap', label: '格式转换' },
  { mode: 'compress', icon: 'compress', label: '媒体压缩' },
  { mode: 'gif', icon: 'film', label: 'GIF 制作器' }
];
</script>

<template>
  <aside class="sidebar">
    <div class="brand">
      <img src="../assets/icon.png" alt="" class="brand-mark" />
      <div><strong>Lythara</strong><span>CONVERT STUDIO</span></div>
    </div>
    <div class="nav-heading">工作台</div>
    <nav id="nav">
      <button
        v-for="item in items"
        :key="item.mode"
        class="nav-item"
        :class="{ active: state.mode === item.mode }"
        @click="setMode(item.mode)"
      >
        <span class="nav-icon"><AppIcon :name="item.icon" /></span>{{ item.label }}<span class="nav-arrow">›</span>
      </button>
    </nav>
    <div class="sidebar-bottom">
      <div class="engine-card">
        <span class="engine-dot"></span>
        <div><b>本地转换引擎</b><small>FFmpeg · 离线处理</small></div>
      </div>
      <div class="credit">© 2026 LytharaLab<br />MIT License · v1.0.0</div>
    </div>
  </aside>
</template>
