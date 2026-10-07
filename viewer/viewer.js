'use strict';
// Small WebGL proof renderer; original asset decoding happens in the local server.
const canvas = document.getElementById('view');
const status = document.getElementById('status');
const gl = canvas.getContext('webgl', {antialias: true, preserveDrawingBuffer: true});
const sub = (a,b) => a.map((v,i)=>v-b[i]);
const dot = (a,b) => a.reduce((s,v,i)=>s+v*b[i],0);
const cross = (a,b) => [a[1]*b[2]-a[2]*b[1],a[2]*b[0]-a[0]*b[2],a[0]*b[1]-a[1]*b[0]];
const norm = a => {const l=Math.hypot(...a)||1;return a.map(v=>v/l);};
function multiply(a,b){const c=new Float32Array(16);for(let j=0;j<4;j++)for(let i=0;i<4;i++)for(let k=0;k<4;k++)c[j*4+i]+=a[k*4+i]*b[j*4+k];return c;}
function perspective(fov,aspect,near,far){const f=1/Math.tan(fov/2),nf=1/(near-far);return new Float32Array([f/aspect,0,0,0,0,f,0,0,0,0,(far+near)*nf,-1,0,0,2*far*near*nf,0]);}
function lookAt(eye,target){const z=norm(sub(eye,target)),x=norm(cross([0,1,0],z)),y=cross(z,x);return new Float32Array([x[0],y[0],z[0],0,x[1],y[1],z[1],0,x[2],y[2],z[2],0,-dot(x,eye),-dot(y,eye),-dot(z,eye),1]);}
function shader(type,source){const s=gl.createShader(type);gl.shaderSource(s,source);gl.compileShader(s);if(!gl.getShaderParameter(s,gl.COMPILE_STATUS))throw Error(gl.getShaderInfoLog(s));return s;}
function program(){const p=gl.createProgram();gl.attachShader(p,shader(gl.VERTEX_SHADER,`attribute vec3 position;attribute vec2 uv;attribute vec3 normal;uniform mat4 mvp;varying vec2 texcoord;varying float light;void main(){gl_Position=mvp*vec4(position,1.);texcoord=uv;light=.72+.28*abs(dot(normal,normalize(vec3(.4,.8,.3))));}`));gl.attachShader(p,shader(gl.FRAGMENT_SHADER,`precision mediump float;varying vec2 texcoord;varying float light;uniform sampler2D image;void main(){vec4 c=texture2D(image,texcoord);if(c.a<.5)discard;gl_FragColor=vec4(c.rgb*light,c.a);}`));gl.linkProgram(p);if(!gl.getProgramParameter(p,gl.LINK_STATUS))throw Error(gl.getProgramInfoLog(p));return p;}
async function start(){
 if(!gl)throw Error('WebGL er ikke tilgjengelig i denne nettleseren.');
 const response=await fetch('/api/model');if(!response.ok)throw Error('Kunne ikke laste modellen.');const model=await response.json();
 const image=new Image();await new Promise((resolve,reject)=>{image.onload=resolve;image.onerror=()=>reject(Error('Kunne ikke laste teksturen.'));image.src='/api/texture.png';});
 const p=program();gl.useProgram(p);
 const texture=gl.createTexture();gl.bindTexture(gl.TEXTURE_2D,texture);gl.texImage2D(gl.TEXTURE_2D,0,gl.RGBA,gl.RGBA,gl.UNSIGNED_BYTE,image);gl.texParameteri(gl.TEXTURE_2D,gl.TEXTURE_MIN_FILTER,gl.LINEAR);gl.texParameteri(gl.TEXTURE_2D,gl.TEXTURE_MAG_FILTER,gl.LINEAR);gl.texParameteri(gl.TEXTURE_2D,gl.TEXTURE_WRAP_S,gl.REPEAT);gl.texParameteri(gl.TEXTURE_2D,gl.TEXTURE_WRAP_T,gl.REPEAT);
 const data=[];for(const triangle of model.triangles){const normal=norm(cross(sub(model.vertices[triangle[1]],model.vertices[triangle[0]]),sub(model.vertices[triangle[2]],model.vertices[triangle[0]])));for(const index of triangle)data.push(...model.vertices[index],...model.uvs[index],...normal);}
 const buffer=gl.createBuffer();gl.bindBuffer(gl.ARRAY_BUFFER,buffer);gl.bufferData(gl.ARRAY_BUFFER,new Float32Array(data),gl.STATIC_DRAW);
 for(const [name,size,offset]of [['position',3,0],['uv',2,12],['normal',3,20]]){const location=gl.getAttribLocation(p,name);gl.enableVertexAttribArray(location);gl.vertexAttribPointer(location,size,gl.FLOAT,false,32,offset);}
 gl.uniform1i(gl.getUniformLocation(p,'image'),0);const mvp=gl.getUniformLocation(p,'mvp');gl.enable(gl.DEPTH_TEST);
 const min=[0,1,2].map(i=>Math.min(...model.vertices.map(v=>v[i]))),max=[0,1,2].map(i=>Math.max(...model.vertices.map(v=>v[i])));
 const center=min.map((v,i)=>(v+max[i])/2),radius=Math.max(...max.map((v,i)=>v-min[i]))*.75;
 let target,yaw,pitch,distance;function reset(){target=center.slice();yaw=.65;pitch=.2;distance=radius*3.2;}reset();document.getElementById('reset').onclick=reset;
 const keys=new Set();window.addEventListener('keydown',e=>{if(['w','a','s','d','q','e','r','shift'].includes(e.key.toLowerCase())){keys.add(e.key.toLowerCase());e.preventDefault();if(e.key.toLowerCase()==='r')reset();}});window.addEventListener('keyup',e=>keys.delete(e.key.toLowerCase()));window.addEventListener('blur',()=>keys.clear());
 let dragging=false,lastX=0,lastY=0;canvas.addEventListener('pointerdown',e=>{dragging=true;lastX=e.clientX;lastY=e.clientY;canvas.setPointerCapture(e.pointerId);});canvas.addEventListener('pointerup',()=>dragging=false);canvas.addEventListener('pointercancel',()=>dragging=false);canvas.addEventListener('pointermove',e=>{if(dragging){yaw-=(e.clientX-lastX)*.006;pitch=Math.max(-1.45,Math.min(1.45,pitch+(e.clientY-lastY)*.006));lastX=e.clientX;lastY=e.clientY;}});canvas.addEventListener('wheel',e=>{distance=Math.max(radius*.25,Math.min(radius*100,distance*Math.exp(e.deltaY*.001)));e.preventDefault();},{passive:false});
 status.textContent=`${model.vertex_count} vertices · ${model.triangle_count} trekanter · ${model.texture_info.width}×${model.texture_info.height} ${model.texture}`;
 window.saProof={model,frames:0,lastGlError:0};let previous=performance.now();
 function draw(time){const dt=Math.min((time-previous)/1000,.05);previous=time;const speed=radius*dt*(keys.has('shift')?4:1);const forward=[-Math.sin(yaw),0,-Math.cos(yaw)],right=[Math.cos(yaw),0,-Math.sin(yaw)];for(const [key,axis,sign] of [['w',forward,1],['s',forward,-1],['d',right,1],['a',right,-1],['e',[0,1,0],1],['q',[0,1,0],-1]])if(keys.has(key))target=target.map((v,i)=>v+axis[i]*sign*speed);
  const ratio=Math.min(devicePixelRatio||1,2),width=Math.round(canvas.clientWidth*ratio),height=Math.round(canvas.clientHeight*ratio);if(canvas.width!==width||canvas.height!==height){canvas.width=width;canvas.height=height;}gl.viewport(0,0,width,height);gl.clearColor(.04,.065,.105,1);gl.clear(gl.COLOR_BUFFER_BIT|gl.DEPTH_BUFFER_BIT);
  const eye=[target[0]+distance*Math.cos(pitch)*Math.sin(yaw),target[1]+distance*Math.sin(pitch),target[2]+distance*Math.cos(pitch)*Math.cos(yaw)];gl.uniformMatrix4fv(mvp,false,multiply(perspective(Math.PI/4,width/height,radius*.01,radius*1000),lookAt(eye,target)));gl.drawArrays(gl.TRIANGLES,0,data.length/8);window.saProof.frames++;window.saProof.lastGlError=gl.getError();requestAnimationFrame(draw);
 }requestAnimationFrame(draw);
}
start().catch(error=>{status.textContent=error.message;status.style.color='#ff9c9c';console.error(error);});
