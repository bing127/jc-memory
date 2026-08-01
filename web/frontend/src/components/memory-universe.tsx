import { useEffect, useRef } from "react";
import * as THREE from "three";

interface MemoryNode {
  position: THREE.Vector3;
  kind: "authority" | "derived" | "surface";
}

const AUTHORITY_NODE_COUNT = 28;
const DERIVED_NODE_COUNT = 220;
const SURFACE_NODE_COUNT = 84;

export function MemoryUniverse() {
  const hostRef = useRef<HTMLElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    const host = hostRef.current;
    const canvas = canvasRef.current;
    if (!host || !canvas || typeof WebGLRenderingContext === "undefined") return;

    const renderer = new THREE.WebGLRenderer({
      alpha: true,
      antialias: true,
      canvas,
      powerPreference: "high-performance",
    });
    renderer.setClearColor(0x030705, 1);
    renderer.setPixelRatio(Math.min(window.devicePixelRatio, 1.75));

    const scene = new THREE.Scene();
    scene.fog = new THREE.FogExp2(0x030705, 0.03);
    const camera = new THREE.PerspectiveCamera(44, 1, 0.1, 100);
    camera.position.set(0, 0, 17);

    const root = new THREE.Group();
    root.position.set(4.4, 0.15, 0);
    scene.add(root);

    const seed = seededRandom(0x4a434d);
    const nodes = createNodes(seed);
    const particleTexture = createParticleTexture();
    const nodeCloud = createNodeCloud(nodes, particleTexture);
    root.add(nodeCloud);

    const edgeGeometry = createEdgeGeometry(nodes, seed);
    const edgeMaterial = new THREE.LineBasicMaterial({
      color: 0xffffff,
      opacity: 0.18,
      transparent: true,
      vertexColors: true,
    });
    const edges = new THREE.LineSegments(edgeGeometry, edgeMaterial);
    root.add(edges);

    const core = new THREE.Mesh(
      new THREE.IcosahedronGeometry(1.18, 2),
      new THREE.MeshBasicMaterial({ color: 0x91ffd0, transparent: true, opacity: 0.3, wireframe: true }),
    );
    root.add(core);

    const coreFill = new THREE.Mesh(
      new THREE.IcosahedronGeometry(1.05, 1),
      new THREE.MeshBasicMaterial({
        blending: THREE.AdditiveBlending,
        color: 0x2ee58c,
        depthWrite: false,
        opacity: 0.09,
        transparent: true,
      }),
    );
    root.add(coreFill);

    const glowTexture = createGlowTexture();
    const coreGlow = new THREE.Sprite(new THREE.SpriteMaterial({
      blending: THREE.AdditiveBlending,
      color: 0x3de698,
      depthWrite: false,
      map: glowTexture,
      opacity: 0.26,
      transparent: true,
    }));
    coreGlow.scale.set(4.6, 4.6, 1);
    root.add(coreGlow);

    const authorityShell = new THREE.Mesh(
      new THREE.IcosahedronGeometry(2, 1),
      new THREE.MeshBasicMaterial({ color: 0x52e9a2, transparent: true, opacity: 0.12, wireframe: true }),
    );
    root.add(authorityShell);

    const surfaceRings = [3.7, 5.1].map((radius, index) => {
      const ring = new THREE.Mesh(
        new THREE.TorusGeometry(radius, 0.009 + index * 0.003, 5, 160),
        new THREE.MeshBasicMaterial({
          color: index === 1 ? 0x50c8e8 : 0x53d99a,
          opacity: 0.18 - index * 0.035,
          transparent: true,
        }),
      );
      ring.rotation.set(0.7 + index * 0.35, index * 0.8, 0.25 + index * 0.4);
      root.add(ring);
      return ring;
    });

    const pulseState = createPulseCloud(nodes, particleTexture, seed);
    root.add(pulseState.points);

    let pointerX = 0;
    let pointerY = 0;
    const onPointerMove = (event: PointerEvent) => {
      pointerX = (event.clientX / window.innerWidth - 0.5) * 2;
      pointerY = (event.clientY / window.innerHeight - 0.5) * 2;
    };
    window.addEventListener("pointermove", onPointerMove, { passive: true });

    const resize = () => {
      const width = Math.max(host.clientWidth, 1);
      const height = Math.max(host.clientHeight, 1);
      renderer.setSize(width, height, false);
      camera.aspect = width / height;
      camera.updateProjectionMatrix();
      root.position.x = width < 760 ? 2.2 : 4.4;
      camera.position.z = width < 760 ? 20 : 17;
    };
    const resizeObserver = new ResizeObserver(resize);
    resizeObserver.observe(host);
    resize();

    const reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    const timer = new THREE.Timer();
    timer.connect(document);
    const render = (timestamp: number) => {
      timer.update(timestamp);
      const elapsed = timer.getElapsed();
      const drift = reducedMotion ? 0 : elapsed;
      root.rotation.y += ((pointerX * 0.11 + drift * 0.018) - root.rotation.y) * 0.018;
      root.rotation.x += ((-pointerY * 0.055 + Math.sin(drift * 0.17) * 0.035) - root.rotation.x) * 0.018;
      core.rotation.x = drift * 0.11;
      core.rotation.y = drift * 0.16;
      coreFill.rotation.x = -drift * 0.08;
      coreFill.rotation.y = drift * 0.13;
      authorityShell.rotation.x = -drift * 0.035;
      authorityShell.rotation.y = drift * 0.05;
      surfaceRings.forEach((ring, index) => {
        ring.rotation.z += reducedMotion ? 0 : (index % 2 === 0 ? 1 : -1) * 0.0007;
      });
      updatePulses(pulseState, drift);
      renderer.render(scene, camera);
    };
    renderer.setAnimationLoop(render);

    return () => {
      renderer.setAnimationLoop(null);
      timer.dispose();
      resizeObserver.disconnect();
      window.removeEventListener("pointermove", onPointerMove);
      particleTexture.dispose();
      glowTexture.dispose();
      disposeObject(root);
      renderer.dispose();
    };
  }, []);

  return (
    <figure
      ref={hostRef}
      aria-label="疆诚记忆三维证据网络"
      className="absolute inset-0 m-0 overflow-hidden"
    >
      <canvas ref={canvasRef} data-memory-universe aria-hidden="true" className="block size-full brightness-110 saturate-125" />
      <figcaption className="pointer-events-none absolute bottom-7 right-7 hidden border-r border-emerald-300/60 pr-3 text-right font-mono text-[9px] leading-5 text-white/45 md:grid">
        <span><i className="mr-2 inline-block size-1.5 bg-emerald-200" />AUTHORITY RECORDS</span>
        <span><i className="mr-2 inline-block size-1.5 bg-emerald-500" />DERIVED GRAPH</span>
        <span><i className="mr-2 inline-block size-1.5 bg-sky-400" />ACTIVE SURFACES</span>
      </figcaption>
    </figure>
  );
}

function createNodes(random: () => number): MemoryNode[] {
  const nodes: MemoryNode[] = [];
  for (let index = 0; index < AUTHORITY_NODE_COUNT; index += 1) {
    nodes.push({ position: sphericalPosition(random, 0.7, 2.2), kind: "authority" });
  }
  for (let index = 0; index < DERIVED_NODE_COUNT; index += 1) {
    nodes.push({ position: sphericalPosition(random, 2.4, 6.6), kind: "derived" });
  }
  for (let index = 0; index < SURFACE_NODE_COUNT; index += 1) {
    const angle = random() * Math.PI * 2;
    const radius = 5.2 + random() * 1.8;
    nodes.push({
      position: new THREE.Vector3(
        Math.cos(angle) * radius,
        (random() - 0.5) * 6.4,
        Math.sin(angle) * radius * 0.72,
      ),
      kind: "surface",
    });
  }
  return nodes;
}

function sphericalPosition(random: () => number, minRadius: number, maxRadius: number) {
  const radius = minRadius + random() * (maxRadius - minRadius);
  const theta = random() * Math.PI * 2;
  const phi = Math.acos(2 * random() - 1);
  return new THREE.Vector3(
    radius * Math.sin(phi) * Math.cos(theta),
    radius * Math.cos(phi) * 0.78,
    radius * Math.sin(phi) * Math.sin(theta),
  );
}

function createNodeCloud(nodes: MemoryNode[], texture: THREE.Texture) {
  const positions = new Float32Array(nodes.length * 3);
  const colors = new Float32Array(nodes.length * 3);
  const sizes = new Float32Array(nodes.length);
  const color = new THREE.Color();
  nodes.forEach((node, index) => {
    node.position.toArray(positions, index * 3);
    color.set(node.kind === "authority" ? 0xb0ffda : node.kind === "surface" ? 0x52dcff : 0x52df92);
    color.toArray(colors, index * 3);
    sizes[index] = node.kind === "authority" ? 2.2 : node.kind === "surface" ? 1.2 : 0.72;
  });
  const geometry = new THREE.BufferGeometry();
  geometry.setAttribute("position", new THREE.BufferAttribute(positions, 3));
  geometry.setAttribute("color", new THREE.BufferAttribute(colors, 3));
  geometry.setAttribute("size", new THREE.BufferAttribute(sizes, 1));
  const material = new THREE.ShaderMaterial({
    blending: THREE.AdditiveBlending,
    depthWrite: false,
    transparent: true,
    vertexColors: true,
    uniforms: {
      pointTexture: { value: texture },
    },
    vertexShader: `
      attribute float size;
      varying vec3 vColor;
      void main() {
        vColor = color;
        vec4 viewPosition = modelViewMatrix * vec4(position, 1.0);
        gl_PointSize = size * (160.0 / max(-viewPosition.z, 1.0));
        gl_Position = projectionMatrix * viewPosition;
      }
    `,
    fragmentShader: `
      uniform sampler2D pointTexture;
      varying vec3 vColor;
      void main() {
        float alpha = texture2D(pointTexture, gl_PointCoord).a;
        if (alpha < 0.02) discard;
        gl_FragColor = vec4(vColor, alpha * 0.92);
      }
    `,
  });
  return new THREE.Points(geometry, material);
}

function createEdgeGeometry(nodes: MemoryNode[], random: () => number) {
  const positions: number[] = [];
  const colors: number[] = [];
  const derivedEnd = AUTHORITY_NODE_COUNT + DERIVED_NODE_COUNT;
  const connect = (fromIndex: number, toIndex: number, surface = false) => {
    const from = nodes[fromIndex];
    const to = nodes[toIndex];
    if (!from || !to) return;
    positions.push(...from.position.toArray(), ...to.position.toArray());
    colors.push(
      surface ? 0.14 : 0.18, surface ? 0.52 : 0.48, surface ? 0.68 : 0.38,
      0.1, surface ? 0.38 : 0.34, surface ? 0.52 : 0.26,
    );
  };

  for (let index = 0; index < AUTHORITY_NODE_COUNT; index += 1) {
    connect(index, (index + 1) % AUTHORITY_NODE_COUNT);
  }
  for (let index = AUTHORITY_NODE_COUNT; index < derivedEnd; index += 3) {
    connect(index, findNearbyNode(nodes, index, AUTHORITY_NODE_COUNT, derivedEnd, random));
  }
  for (let index = derivedEnd; index < nodes.length; index += 4) {
    connect(index, findNearbyNode(nodes, index, AUTHORITY_NODE_COUNT, derivedEnd, random), true);
  }
  const geometry = new THREE.BufferGeometry();
  geometry.setAttribute("position", new THREE.Float32BufferAttribute(positions, 3));
  geometry.setAttribute("color", new THREE.Float32BufferAttribute(colors, 3));
  return geometry;
}

function findNearbyNode(
  nodes: MemoryNode[],
  sourceIndex: number,
  rangeStart: number,
  rangeEnd: number,
  random: () => number,
) {
  const source = nodes[sourceIndex];
  let bestIndex = rangeStart;
  let bestDistance = Number.POSITIVE_INFINITY;
  for (let attempt = 0; attempt < 18; attempt += 1) {
    const candidateIndex = rangeStart + Math.floor(random() * Math.max(rangeEnd - rangeStart, 1));
    if (candidateIndex === sourceIndex) continue;
    const candidate = nodes[candidateIndex];
    if (!source || !candidate) continue;
    const distance = source.position.distanceToSquared(candidate.position);
    if (distance < bestDistance) {
      bestDistance = distance;
      bestIndex = candidateIndex;
    }
  }
  return bestIndex;
}

interface PulseState {
  points: THREE.Points;
  positions: THREE.BufferAttribute;
  paths: Array<{ from: THREE.Vector3; to: THREE.Vector3; phase: number; speed: number }>;
}

function createPulseCloud(nodes: MemoryNode[], texture: THREE.Texture, random: () => number): PulseState {
  const paths = Array.from({ length: 24 }, () => {
    const fromIndex = AUTHORITY_NODE_COUNT + Math.floor(random() * DERIVED_NODE_COUNT);
    const toIndex = findNearbyNode(
      nodes,
      fromIndex,
      AUTHORITY_NODE_COUNT,
      AUTHORITY_NODE_COUNT + DERIVED_NODE_COUNT,
      random,
    );
    const from = nodes[fromIndex]?.position ?? new THREE.Vector3();
    const to = nodes[toIndex]?.position ?? new THREE.Vector3();
    return { from, to, phase: random(), speed: 0.035 + random() * 0.075 };
  });
  const geometry = new THREE.BufferGeometry();
  const positions = new THREE.BufferAttribute(new Float32Array(paths.length * 3), 3);
  geometry.setAttribute("position", positions);
  const material = new THREE.PointsMaterial({
    blending: THREE.AdditiveBlending,
    color: 0x96ffd0,
    depthWrite: false,
    map: texture,
    opacity: 0.95,
    size: 0.26,
    transparent: true,
  });
  return { points: new THREE.Points(geometry, material), positions, paths };
}

function updatePulses(state: PulseState, elapsed: number) {
  state.paths.forEach((path, index) => {
    const progress = (path.phase + elapsed * path.speed) % 1;
    const point = path.from.clone().lerp(path.to, easeInOut(progress));
    state.positions.setXYZ(index, point.x, point.y, point.z);
  });
  state.positions.needsUpdate = true;
}

function createParticleTexture() {
  const canvas = document.createElement("canvas");
  canvas.width = 64;
  canvas.height = 64;
  const context = canvas.getContext("2d");
  if (context) {
    context.clearRect(0, 0, 64, 64);
    context.fillStyle = "rgba(103, 239, 170, 0.12)";
    context.beginPath();
    context.arc(32, 32, 30, 0, Math.PI * 2);
    context.fill();
    context.strokeStyle = "rgba(185, 255, 218, 0.9)";
    context.lineWidth = 2;
    context.beginPath();
    context.arc(32, 32, 12, 0, Math.PI * 2);
    context.stroke();
    context.fillStyle = "rgba(220, 255, 237, 1)";
    context.beginPath();
    context.arc(32, 32, 3, 0, Math.PI * 2);
    context.fill();
  }
  const texture = new THREE.CanvasTexture(canvas);
  texture.colorSpace = THREE.SRGBColorSpace;
  return texture;
}

function createGlowTexture() {
  const canvas = document.createElement("canvas");
  canvas.width = 128;
  canvas.height = 128;
  const context = canvas.getContext("2d");
  if (context) {
    const glow = context.createRadialGradient(64, 64, 0, 64, 64, 64);
    glow.addColorStop(0, "rgba(170, 255, 211, 0.95)");
    glow.addColorStop(0.15, "rgba(72, 236, 150, 0.5)");
    glow.addColorStop(0.48, "rgba(38, 189, 112, 0.14)");
    glow.addColorStop(1, "rgba(0, 0, 0, 0)");
    context.fillStyle = glow;
    context.fillRect(0, 0, 128, 128);
  }
  const texture = new THREE.CanvasTexture(canvas);
  texture.colorSpace = THREE.SRGBColorSpace;
  return texture;
}

function seededRandom(initialSeed: number) {
  let seed = initialSeed >>> 0;
  return () => {
    seed = (seed * 1664525 + 1013904223) >>> 0;
    return seed / 0x1_0000_0000;
  };
}

function easeInOut(value: number) {
  return value < 0.5 ? 2 * value * value : 1 - ((-2 * value + 2) ** 2) / 2;
}

function disposeObject(root: THREE.Object3D) {
  root.traverse((object) => {
    if ("geometry" in object && object.geometry instanceof THREE.BufferGeometry) {
      object.geometry.dispose();
    }
    if ("material" in object) {
      const material = object.material as THREE.Material | THREE.Material[];
      (Array.isArray(material) ? material : [material]).forEach((item) => item.dispose());
    }
  });
}
