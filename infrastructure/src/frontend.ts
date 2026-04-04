import * as aws from "@pulumi/aws";
import * as cloudflare from "@pulumi/cloudflare";
import * as pulumi from "@pulumi/pulumi";
import { domain, stack, zireaelZoneId } from "./config";
import { apiGateway } from "./network";

// ---------------------------------------------------------------------------
// S3 Bucket
// ---------------------------------------------------------------------------
export const siteBucket = new aws.s3.Bucket("sentinel-frontend-bucket", {});

new aws.s3.BucketPublicAccessBlock("sentinel-frontend-block", {
  bucket: siteBucket.id,
  blockPublicAcls: true,
  blockPublicPolicy: true,
  ignorePublicAcls: true,
  restrictPublicBuckets: true,
});

const oac = new aws.cloudfront.OriginAccessControl("sentinel-oac", {
  name: `sentinel-${stack}-oac`,
  originAccessControlOriginType: "s3",
  signingBehavior: "always",
  signingProtocol: "sigv4",
});

// ---------------------------------------------------------------------------
// ACM Certificate
// ---------------------------------------------------------------------------
const cert = new aws.acm.Certificate("sentinel-cert", {
  domainName: domain,
  validationMethod: "DNS",
});

const certVal = cert.domainValidationOptions.apply((opts) => opts[0]);
const certValRecord = new cloudflare.Record("sentinel-cert-validation", {
  zoneId: zireaelZoneId,
  name: certVal.apply((v) => v.resourceRecordName),
  type: certVal.apply((v) => v.resourceRecordType),
  content: certVal.apply((v) => v.resourceRecordValue),
  ttl: 60,
});

const certValidated = new aws.acm.CertificateValidation(
  "sentinel-cert-validated",
  {
    certificateArn: cert.arn,
    validationRecordFqdns: [certValRecord.name],
  },
);

// ---------------------------------------------------------------------------
// API Gateway origin domain (extract host from invoke URL)
// ---------------------------------------------------------------------------
const apiOriginDomain = apiGateway.apiEndpoint.apply((url) => {
  const parsed = new URL(url);
  return parsed.hostname;
});

// ---------------------------------------------------------------------------
// CloudFront Distribution
// ---------------------------------------------------------------------------
export const cdn = new aws.cloudfront.Distribution("sentinel-cdn", {
  enabled: true,
  defaultRootObject: "index.html",
  aliases: [domain],
  origins: [
    {
      domainName: siteBucket.bucketRegionalDomainName,
      originId: "s3",
      originAccessControlId: oac.id,
    },
    {
      domainName: apiOriginDomain,
      originId: "api",
      customOriginConfig: {
        httpPort: 80,
        httpsPort: 443,
        originProtocolPolicy: "https-only",
        originSslProtocols: ["TLSv1.2"],
      },
    },
  ],
  defaultCacheBehavior: {
    targetOriginId: "s3",
    viewerProtocolPolicy: "redirect-to-https",
    allowedMethods: ["GET", "HEAD", "OPTIONS"],
    cachedMethods: ["GET", "HEAD"],
    forwardedValues: { queryString: false, cookies: { forward: "none" } },
    compress: true,
  },
  orderedCacheBehaviors: [
    {
      pathPattern: "/api/*",
      targetOriginId: "api",
      viewerProtocolPolicy: "redirect-to-https",
      allowedMethods: [
        "DELETE",
        "GET",
        "HEAD",
        "OPTIONS",
        "PATCH",
        "POST",
        "PUT",
      ],
      cachedMethods: ["GET", "HEAD"],
      // CachingDisabled managed policy — API responses should not be cached
      cachePolicyId: "4135ea2d-6df8-44a3-9df3-4b5a84be39ad",
      // AllViewerExceptHostHeader — pass headers/query through, rewrite Host
      originRequestPolicyId: "b689b0a8-53d0-40ab-baf2-68738e2966ac",
      compress: true,
    },
  ],
  customErrorResponses: [
    { errorCode: 404, responseCode: 200, responsePagePath: "/index.html" },
    { errorCode: 403, responseCode: 200, responsePagePath: "/index.html" },
  ],
  viewerCertificate: {
    acmCertificateArn: certValidated.certificateArn,
    sslSupportMethod: "sni-only",
    minimumProtocolVersion: "TLSv1.2_2021",
  },
  restrictions: { geoRestriction: { restrictionType: "none" } },
});

// S3 bucket policy: allow CloudFront OAC
new aws.s3.BucketPolicy("sentinel-frontend-policy", {
  bucket: siteBucket.id,
  policy: pulumi.all([siteBucket.arn, cdn.arn]).apply(([bucketArn, cdnArn]) =>
    JSON.stringify({
      Version: "2012-10-17",
      Statement: [
        {
          Effect: "Allow",
          Principal: { Service: "cloudfront.amazonaws.com" },
          Action: "s3:GetObject",
          Resource: `${bucketArn}/*`,
          Condition: { StringEquals: { "AWS:SourceArn": cdnArn } },
        },
      ],
    }),
  ),
});

// Cloudflare DNS
new cloudflare.Record("sentinel-dns", {
  zoneId: zireaelZoneId,
  name: domain,
  type: "CNAME",
  content: cdn.domainName,
  ttl: 1,
  proxied: false,
});
