/** State regions share their parent sheet; internal joins own no bevel. */
export function glassConnectedRegions(liquid = false): string {
  return `
// Connected regions tint their parent's sheet without owning any reflections.
// Negative native roles are the two inset stepper cells, part of their input.
if(family==9.0 || data.w< -2.5) {
  vec2 aa=max(fwidth(position),vec2(.6));
  float selected=clamp(data.w,0.0,1.0);
  float alpha=(selected*.20+data.x*.065+data.y*.04)*(1.0-data.z*.6);
  float inset=1.0;
  float divider=0.0;
  float focusDepth=min(min(position.x,size.x-position.x),min(position.y,size.y-position.y));
  // Inset menu rows round their state tint inside the parent's continuous sheet.
  if(p_body.w<-.5) {
    float radius=min(${liquid ? "12.0" : "8.0"}*unitScale,minor*.5);
    vec2 q=abs(position-size*.5)-(size*.5-radius);
    float rounded=length(max(q,0.0))+min(max(q.x,q.y),0.0)-radius;
    float edgeAA=max(fwidth(rounded),.45);
    inset=1.0-smoothstep(-edgeAA*.5,edgeAA*.5,rounded);
    focusDepth=-rounded;
  }
  if(p_body.w>0.5 && p_body.w<4.5) {
    bool left=p_body.w<1.5 || p_body.w>3.5;
    bool right=p_body.w>2.5;
    float radius=${liquid ? "minor*.5" : "8.0*unitScale"};
    vec2 q=abs(position-size*.5)-(size*.5-radius);
    float rounded=length(max(q,0.0))+min(max(q.x,q.y),0.0)-radius;
    if((left && position.x<size.x*.5)||(right && position.x>size.x*.5)) {
      inset=smoothstep(.5,2.5,-rounded);
      focusDepth=-rounded;
    }
    inset*=smoothstep(.5,2.5,min(position.y,size.y-position.y));
    if(!left) divider=1.0-smoothstep(.4,.4+aa.x,position.x);
  }
  // Tabs tint the shared panel. Only the upper corners curve; the lower edge
  // stays joined to the content, with no second outline or raised capsule.
  if(p_body.w>5.5) {
    float radius=min(8.0*unitScale,minor*.5);
    vec2 q=abs(position-size*.5)-(size*.5-radius);
    float rounded=length(max(q,0.0))+min(max(q.x,q.y),0.0)-radius;
    if(position.y<radius) {
      inset=1.0-smoothstep(-.5,.5,rounded);
      focusDepth=-rounded;
    }
    divider=(1.0-smoothstep(.4,.4+aa.x,size.x-position.x))*inset;
  }
  if(p_body.w>4.5 && p_body.w<5.5) divider=1.0-smoothstep(.4,.4+aa.y,size.y-position.y);
  if(data.w< -2.5) divider=1.0-smoothstep(.4,.4+aa.x,data.w< -3.5?position.x:size.x-position.x);
  ${
    liquid
      ? `// Step cells share the input capsule at their exterior end only.
  if(data.w< -2.5) {
    bool left=data.w> -3.5;
    float radius=size.y*.5;
    vec2 center=vec2(left?radius:size.x-radius,radius);
    float outer=left?position.x-radius:radius-(size.x-position.x);
    if((left && outer<0.0)||(!left && outer>0.0))
      inset=1.0-smoothstep(-.5,.5,length(position-center)-radius);
  }`
      : ""
  }
  alpha=max(alpha*inset,divider*.16);
  if(data.w>3.5) {
    // Keyboard focus follows the same joined contour, inset within the sheet.
    // A straight underline would escape the capsule at its rounded ends.
    float edgeAA=max(fwidth(focusDepth),.45);
    alpha=(1.0-smoothstep(.45*unitScale,.45*unitScale+edgeAA,
      abs(focusDepth-2.5*unitScale)))*inset*.58;
  }
  return vec4(mix(pale,p_glassTint.rgb,.35),alpha);
}
`;
}
