#version 450

layout(binding = 1) uniform sampler2D texSampler;

layout(location = 0) in vec4 vColor;
layout(location = 1) in vec2 vTexCoord;

layout(location = 0) out vec4 fragColor;

void main() {
	vec4 s = texture(texSampler, vTexCoord);
	fragColor = vec4(s.r, s.g, s.b, s.a);
	//fragColor = vColor;
	//fragColor = vColor * vec4(s.g, s.b, s.a, 1.0);
}