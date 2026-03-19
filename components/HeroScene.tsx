'use client';

/**
 * HeroScene — Three.js orbital network visualization.
 *
 * Renders an immersive 3-D particle network that represents agents
 * communicating over the 0x402 protocol and Kafka pub-sub backbone:
 *
 * - Core sphere: the Stellar ledger / smart contract hub
 * - Orbital rings: three Kafka topic planes (payment, agent, chain)
 * - Nodes on the rings: individual agents
 * - Animated pulses along connecting lines: live payment flows
 * - Background starfield: the wider DeFi ecosystem
 */

import { useEffect, useRef } from 'react';
import * as THREE from 'three';

// ── Colour palette (matches global teal theme) ───────────────────────────────
const C = {
  teal:   0x00ffe5,
  violet: 0x7b61ff,
  amber:  0xf59e0b,
  white:  0xffffff,
  bg:     0x050508,
};

// ── Scene parameters ──────────────────────────────────────────────────────────
const STAR_COUNT      = 1200;
const RING_NODES      = [8, 12, 16]; // nodes per ring
const RING_RADII      = [2.4, 3.6, 5.0];
const RING_TILTS      = [0.3, -0.5, 0.7];
const NODE_SIZE       = 0.06;
const CORE_SIZE       = 0.45;
const PULSE_SPEED     = 0.008;
const AUTO_ROTATE     = 0.0015;

// ── Component ─────────────────────────────────────────────────────────────────

export default function HeroScene() {
  const mountRef  = useRef<HTMLDivElement>(null);
  const frameRef  = useRef<number>(0);
  const sceneRef  = useRef<THREE.Scene | null>(null);

  useEffect(() => {
    if (!mountRef.current) return;
    const mount = mountRef.current;

    // ── Renderer ─────────────────────────────────────────────────────────────
    const renderer = new THREE.WebGLRenderer({ antialias: true, alpha: true });
    renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2));
    renderer.setSize(mount.clientWidth, mount.clientHeight);
    renderer.setClearColor(0x000000, 0);
    mount.appendChild(renderer.domElement);

    // ── Scene / Camera ────────────────────────────────────────────────────────
    const scene  = new THREE.Scene();
    sceneRef.current = scene;
    const camera = new THREE.PerspectiveCamera(
      55,
      mount.clientWidth / mount.clientHeight,
      0.1,
      100
    );
    camera.position.set(0, 1.5, 9);
    camera.lookAt(0, 0, 0);

    // ── Lights ────────────────────────────────────────────────────────────────
    scene.add(new THREE.AmbientLight(0xffffff, 0.3));
    const pointLight1 = new THREE.PointLight(C.teal, 2.5, 12);
    pointLight1.position.set(3, 3, 3);
    scene.add(pointLight1);
    const pointLight2 = new THREE.PointLight(C.violet, 1.8, 10);
    pointLight2.position.set(-3, -2, 2);
    scene.add(pointLight2);

    // ── Root group (rotates everything together) ──────────────────────────────
    const root = new THREE.Group();
    scene.add(root);

    // ── Core sphere (Stellar ledger) ──────────────────────────────────────────
    const coreMat = new THREE.MeshStandardMaterial({
      color:        C.teal,
      emissive:     C.teal,
      emissiveIntensity: 0.6,
      roughness:    0.2,
      metalness:    0.9,
      wireframe:    false,
    });
    const coreGeo  = new THREE.IcosahedronGeometry(CORE_SIZE, 4);
    const coreMesh = new THREE.Mesh(coreGeo, coreMat);
    root.add(coreMesh);

    // Inner glow (slightly larger wireframe)
    const glowMat  = new THREE.MeshBasicMaterial({
      color:       C.teal, wireframe: true, transparent: true, opacity: 0.12
    });
    root.add(new THREE.Mesh(new THREE.IcosahedronGeometry(CORE_SIZE * 1.18, 2), glowMat));

    // ── Helper: build a torus ring ─────────────────────────────────────────────
    function makeRing(radius: number, tilt: number, color: number): THREE.Group {
      const group   = new THREE.Group();
      group.rotation.x = tilt;

      const torusGeo = new THREE.TorusGeometry(radius, 0.006, 8, 120);
      const torusMat = new THREE.MeshBasicMaterial({
        color, transparent: true, opacity: 0.25
      });
      group.add(new THREE.Mesh(torusGeo, torusMat));
      return group;
    }

    // ── Orbital rings (Kafka topics) ─────────────────────────────────────────
    const ringColors  = [C.teal, C.violet, C.amber];
    const ringGroups: THREE.Group[] = [];

    RING_RADII.forEach((r, idx) => {
      const rg = makeRing(r, RING_TILTS[idx], ringColors[idx]);
      root.add(rg);
      ringGroups.push(rg);
    });

    // ── Agent nodes on each ring ──────────────────────────────────────────────
    const nodeMeshes: THREE.Mesh[] = [];

    RING_RADII.forEach((radius, ringIdx) => {
      const count = RING_NODES[ringIdx];
      const tilt  = RING_TILTS[ringIdx];
      const color = ringColors[ringIdx];

      for (let j = 0; j < count; j++) {
        const angle = (j / count) * Math.PI * 2;
        const x = Math.cos(angle) * radius;
        const z = Math.sin(angle) * radius;
        // Apply tilt (rotate around X axis)
        const y = z * Math.sin(tilt);
        const zt = z * Math.cos(tilt);

        const geo  = new THREE.SphereGeometry(NODE_SIZE, 8, 8);
        const mat  = new THREE.MeshStandardMaterial({
          color,
          emissive:          color,
          emissiveIntensity: 0.8,
          roughness:         0.1,
          metalness:         1.0,
        });
        const mesh = new THREE.Mesh(geo, mat);
        mesh.position.set(x, y, zt);
        root.add(mesh);
        nodeMeshes.push(mesh);
      }
    });

    // ── Connection lines (core → each node) ───────────────────────────────────
    const lineMat = new THREE.LineBasicMaterial({
      color: C.teal, transparent: true, opacity: 0.08
    });

    nodeMeshes.forEach(node => {
      const points = [new THREE.Vector3(0, 0, 0), node.position.clone()];
      const geo    = new THREE.BufferGeometry().setFromPoints(points);
      root.add(new THREE.Line(geo, lineMat));
    });

    // ── Animated payment pulses ───────────────────────────────────────────────
    interface Pulse {
      mesh:  THREE.Mesh;
      from:  THREE.Vector3;
      to:    THREE.Vector3;
      t:     number;
      speed: number;
    }

    const pulseMat = new THREE.MeshBasicMaterial({ color: C.teal });
    const pulses: Pulse[] = [];

    function spawnPulse() {
      const target = nodeMeshes[Math.floor(Math.random() * nodeMeshes.length)];
      const geo    = new THREE.SphereGeometry(0.04, 6, 6);
      const mesh   = new THREE.Mesh(geo, pulseMat.clone());
      mesh.position.set(0, 0, 0);
      root.add(mesh);
      pulses.push({
        mesh,
        from:  new THREE.Vector3(0, 0, 0),
        to:    target.position.clone(),
        t:     0,
        speed: PULSE_SPEED * (0.6 + Math.random() * 0.8),
      });
    }

    // Seed some initial pulses
    for (let i = 0; i < 5; i++) spawnPulse();

    // ── Starfield ─────────────────────────────────────────────────────────────
    const starGeo     = new THREE.BufferGeometry();
    const starPos     = new Float32Array(STAR_COUNT * 3);
    for (let i = 0; i < STAR_COUNT; i++) {
      starPos[i * 3 + 0] = (Math.random() - 0.5) * 80;
      starPos[i * 3 + 1] = (Math.random() - 0.5) * 80;
      starPos[i * 3 + 2] = (Math.random() - 0.5) * 80;
    }
    starGeo.setAttribute('position', new THREE.BufferAttribute(starPos, 3));
    const starMat = new THREE.PointsMaterial({
      color: 0xffffff, size: 0.07, transparent: true, opacity: 0.55
    });
    scene.add(new THREE.Points(starGeo, starMat));

    // ── Resize handler ────────────────────────────────────────────────────────
    function onResize() {
      if (!mount) return;
      const w = mount.clientWidth;
      const h = mount.clientHeight;
      camera.aspect = w / h;
      camera.updateProjectionMatrix();
      renderer.setSize(w, h);
    }
    window.addEventListener('resize', onResize);

    // ── Mouse parallax ────────────────────────────────────────────────────────
    const mouse = { x: 0, y: 0 };
    function onMouseMove(e: MouseEvent) {
      mouse.x = (e.clientX / window.innerWidth  - 0.5) * 2;
      mouse.y = (e.clientY / window.innerHeight - 0.5) * 2;
    }
    window.addEventListener('mousemove', onMouseMove);

    // ── Animation loop ────────────────────────────────────────────────────────
    let tick = 0;

    function animate() {
      frameRef.current = requestAnimationFrame(animate);
      tick++;

      // Slow auto-rotation
      root.rotation.y += AUTO_ROTATE;

      // Gentle parallax tilt
      root.rotation.x += (mouse.y * 0.08 - root.rotation.x) * 0.04;
      root.rotation.z += (-mouse.x * 0.04 - root.rotation.z) * 0.04;

      // Core pulsing
      const pulse = 1 + Math.sin(tick * 0.04) * 0.06;
      coreMesh.scale.setScalar(pulse);

      // Move payment pulses
      for (let i = pulses.length - 1; i >= 0; i--) {
        const p = pulses[i];
        p.t += p.speed;
        if (p.t >= 1) {
          root.remove(p.mesh);
          pulses.splice(i, 1);
          // Spawn a new pulse to replace it
          spawnPulse();
        } else {
          p.mesh.position.lerpVectors(p.from, p.to, easeInOut(p.t));
          // Fade out near destination
          (p.mesh.material as THREE.MeshBasicMaterial).opacity =
            Math.sin(p.t * Math.PI);
          (p.mesh.material as THREE.MeshBasicMaterial).transparent = true;
        }
      }

      // Slowly vary ring opacities
      ringGroups.forEach((rg, i) => {
        rg.rotation.z = Math.sin(tick * 0.005 + i * 1.2) * 0.02;
      });

      renderer.render(scene, camera);
    }

    animate();

    return () => {
      cancelAnimationFrame(frameRef.current);
      window.removeEventListener('resize', onResize);
      window.removeEventListener('mousemove', onMouseMove);
      renderer.dispose();
      if (mount.contains(renderer.domElement)) {
        mount.removeChild(renderer.domElement);
      }
    };
  }, []);

  return (
    <div
      ref={mountRef}
      className="w-full h-full"
      aria-hidden="true"
    />
  );
}

// ── Math helpers ──────────────────────────────────────────────────────────────

function easeInOut(t: number): number {
  return t < 0.5 ? 2 * t * t : -1 + (4 - 2 * t) * t;
}
