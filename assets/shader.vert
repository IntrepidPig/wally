#version 450

layout(binding = 0) uniform NANI {
	mat4 model;
	mat4 view;
	mat4 projection;
} mvp;

layout(location = 0) in vec3 pos;
layout(location = 1) in vec4 col;
layout(location = 2) in vec2 tex;

layout(location = 0) out vec4 vColor;
layout(location = 1) out vec2 vTexCoord;

void main() {
	gl_Position = mvp.projection * mvp.view * mvp.model * vec4(pos, 1.0);
	vColor = col;
	vTexCoord = tex;
}